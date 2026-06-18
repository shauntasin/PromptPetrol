use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Context as CanvasContext, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::claude_import::ClaudeImportDiagnostics;
use crate::codex_import::{CodexImportCache, codex_session_snapshot};

const APP_NAME: &str = "PromptPetrol";

/// One limit readout rendered as a boxed-digit odometer (drum + bar + reset).
struct Metric {
    /// Full title for the spacious (tall) layout, e.g. "CLAUDE · 5h".
    title: String,
    /// Short title for the compact layout, e.g. "CL 5h".
    short: String,
    /// `None` means the value is unavailable; the drum shows `--`.
    percent: Option<f64>,
    note: String,
}

/// The bottom context-window readout (a single full-width bar).
struct Context {
    percent: f64,
    detail: String,
}

pub(crate) fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();

    if area.width < 24 || area.height < 6 {
        frame.render_widget(
            Paragraph::new(format!("{APP_NAME}: enlarge terminal")),
            area,
        );
        return;
    }

    let title = format!(" {APP_NAME} ");
    let block = rounded_block(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (claude_metrics, codex_metrics) = collect_metrics(&app.claude_cache, &app.codex_cache);
    let context = collect_context(&app.codex_cache);

    // Spacious terminals get the analog dial cluster; smaller ones get the
    // compact odometer bars.
    if inner.width >= 72 && inner.height >= 18 {
        render_cluster(
            frame,
            inner,
            &claude_metrics,
            &codex_metrics,
            context.as_ref(),
        );
    } else {
        render_dashboard(
            frame,
            inner,
            &claude_metrics,
            &codex_metrics,
            context.as_ref(),
        );
    }

    if app.show_help {
        draw_help_overlay(frame);
    }
}

/// BMW-style instrument cluster: two big dials (5h limits) flanked by two small
/// dials (weekly limits), with a center context readout.
fn render_cluster(
    frame: &mut Frame<'_>,
    area: Rect,
    claude: &[Metric],
    codex: &[Metric],
    context: Option<&Context>,
) {
    // Columns: small | BIG | center | BIG | small.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(16),
            Constraint::Percentage(28),
            Constraint::Percentage(12),
            Constraint::Percentage(28),
            Constraint::Percentage(16),
        ])
        .split(area);

    let claude_5h = claude.first();
    let claude_wk = claude.get(1);
    let codex_5h = codex.first();
    let codex_wk = codex.get(1);

    if let Some(m) = claude_wk {
        render_dial(frame, cols[0], m, DialSize::Small);
    }
    if let Some(m) = claude_5h {
        render_dial(frame, cols[1], m, DialSize::Big);
    }
    render_center(frame, cols[2], context);
    if let Some(m) = codex_5h {
        render_dial(frame, cols[3], m, DialSize::Big);
    }
    if let Some(m) = codex_wk {
        render_dial(frame, cols[4], m, DialSize::Small);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DialSize {
    Big,
    Small,
}

/// Draws one round dial: glowing rim, tick arc, red redline zone, needle, and a
/// center hub with the percentage and label.
fn render_dial(frame: &mut Frame<'_>, area: Rect, metric: &Metric, size: DialSize) {
    if area.width < 8 || area.height < 5 {
        // Too small for a dial; degrade to a compact row.
        render_compact_metric(frame, area, metric);
        return;
    }

    let pct = metric.percent.unwrap_or(0.0);
    let ratio = (pct / 100.0).clamp(0.0, 1.0);
    let needle_color = metric.percent.map(percent_color).unwrap_or(Color::DarkGray);

    // The sweep runs from 225° (bottom-left) clockwise to -45° (bottom-right),
    // a 270° span — the classic automotive gauge layout.
    let start_deg = 225.0_f64;
    let span_deg = 270.0_f64;
    // Redline zone covers the top ~20% of the sweep.
    let redline_start = 0.80_f64;

    // Terminal cells are about twice as tall as wide; widen x-bounds so the dial
    // reads as a circle rather than an ellipse.
    let canvas = Canvas::default()
        .x_bounds([-1.4, 1.4])
        .y_bounds([-1.1, 1.1])
        .paint(move |ctx| {
            draw_dial_face(ctx, start_deg, span_deg, redline_start, size);
            draw_needle(ctx, start_deg, span_deg, ratio, needle_color);
        });
    frame.render_widget(canvas, area);

    // Center hub text: big percentage + label, overlaid on the dial center.
    let pct_text = match metric.percent {
        Some(p) => format!("{:.0}%", p.min(999.0)),
        None => "--".into(),
    };
    let label = if size == DialSize::Big {
        metric.title.clone()
    } else {
        metric.short.clone()
    };

    let hub_y = area.y + area.height / 2;
    let hub = Rect {
        x: area.x,
        y: hub_y,
        width: area.width,
        height: 2.min(area.y + area.height - hub_y),
    };
    let mut lines = vec![
        Line::from(Span::styled(
            pct_text,
            Style::default()
                .fg(needle_color)
                .add_modifier(Modifier::BOLD),
        ))
        .centered(),
    ];
    if hub.height >= 2 {
        lines.push(
            Line::from(Span::styled(
                truncate(&label, area.width as usize),
                Style::default().fg(Color::Cyan),
            ))
            .centered(),
        );
    }
    frame.render_widget(Paragraph::new(lines), hub);

    // Reset / note below the dial.
    if !is_blank(&metric.note) {
        let note_y = area.y + area.height.saturating_sub(1);
        let note_area = Rect {
            x: area.x,
            y: note_y,
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(
                Line::from(Span::styled(
                    truncate(&metric.note, area.width as usize),
                    Style::default().fg(Color::DarkGray),
                ))
                .centered(),
            ),
            note_area,
        );
    }
}

/// Paints the static dial face: rim ring, tick marks, and the red redline arc.
fn draw_dial_face(
    ctx: &mut CanvasContext,
    start_deg: f64,
    span_deg: f64,
    redline_start: f64,
    size: DialSize,
) {
    let rim_color = Color::Cyan;

    // Thin glowing rim: a single-radius ring of dots around the full circle.
    let rim_steps = if size == DialSize::Big { 240 } else { 160 };
    let rim_r = 1.0;
    for i in 0..rim_steps {
        let a = (i as f64 / rim_steps as f64 * 360.0).to_radians();
        let b = ((i + 1) as f64 / rim_steps as f64 * 360.0).to_radians();
        ctx.draw(&CanvasLine {
            x1: a.cos() * rim_r,
            y1: a.sin() * rim_r,
            x2: b.cos() * rim_r,
            y2: b.sin() * rim_r,
            color: rim_color,
        });
    }

    // Tick marks along the 270° sweep; every 5th tick is a long "major" tick.
    let tick_count = if size == DialSize::Big { 40 } else { 20 };
    for i in 0..=tick_count {
        let t = i as f64 / tick_count as f64;
        let a = (start_deg - span_deg * t).to_radians();
        let major = i % 5 == 0;
        let r_in = if major { 0.74 } else { 0.82 };
        let in_redline = t >= redline_start;
        let color = if in_redline {
            Color::Red
        } else {
            Color::Rgb(255, 150, 60)
        };
        ctx.draw(&CanvasLine {
            x1: a.cos() * r_in,
            y1: a.sin() * r_in,
            x2: a.cos() * 0.90,
            y2: a.sin() * 0.90,
            color,
        });
    }

    // Thick red redline arc just inside the ticks at the top of the sweep.
    let arc_steps = 40;
    for i in 0..=arc_steps {
        let t = redline_start + (1.0 - redline_start) * (i as f64 / arc_steps as f64);
        let a = (start_deg - span_deg * t).to_radians();
        ctx.draw(&CanvasLine {
            x1: a.cos() * 0.64,
            y1: a.sin() * 0.64,
            x2: a.cos() * 0.70,
            y2: a.sin() * 0.70,
            color: Color::Red,
        });
    }
}

/// Draws the needle from the hub out to the current ratio along the sweep.
fn draw_needle(ctx: &mut CanvasContext, start_deg: f64, span_deg: f64, ratio: f64, color: Color) {
    let a = (start_deg - span_deg * ratio).to_radians();
    // Bold needle: several parallel lines offset perpendicular to the needle so
    // it reads as a solid pointer rather than a thin hairline.
    let (nx, ny) = (a.cos(), a.sin());
    let (px, py) = (-ny, nx); // perpendicular unit vector
    for k in -2..=2 {
        let off = k as f64 * 0.015;
        ctx.draw(&CanvasLine {
            x1: -nx * 0.16 + px * off,
            y1: -ny * 0.16 + py * off,
            x2: nx * 0.80 + px * off,
            y2: ny * 0.80 + py * off,
            color,
        });
    }
}

/// Center column: context-window readout, styled like the BMW info display.
fn render_center(frame: &mut Frame<'_>, area: Rect, context: Option<&Context>) {
    let mut lines = vec![Line::raw("")];
    match context {
        Some(ctx) => {
            lines.push(
                Line::from(Span::styled(
                    "CONTEXT",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .centered(),
            );
            lines.push(
                Line::from(Span::styled(
                    format!("{:.0}%", ctx.percent.min(999.0)),
                    Style::default()
                        .fg(percent_color(ctx.percent))
                        .add_modifier(Modifier::BOLD),
                ))
                .centered(),
            );
            lines.push(
                Line::from(Span::styled(
                    ctx.detail.clone(),
                    Style::default().fg(Color::DarkGray),
                ))
                .centered(),
            );
        }
        None => {
            lines.push(
                Line::from(Span::styled(
                    "PromptPetrol",
                    Style::default().fg(Color::Cyan),
                ))
                .centered(),
            );
        }
    }
    // Vertically center the block.
    let pad = (area.height as usize).saturating_sub(lines.len()) / 2;
    let mut padded = vec![Line::raw(""); pad];
    padded.extend(lines);
    frame.render_widget(Paragraph::new(padded), area);
}

fn collect_metrics(
    claude: &ClaudeImportDiagnostics,
    codex: &CodexImportCache,
) -> (Vec<Metric>, Vec<Metric>) {
    let cl = claude.limits.as_ref();
    let claude_metrics = vec![
        Metric {
            title: "CLAUDE · 5h".into(),
            short: "CL 5h".into(),
            percent: cl.and_then(|l| l.primary.as_ref()).map(|l| l.used_percent),
            note: reset_note(
                cl.and_then(|l| l.primary.as_ref())
                    .and_then(|l| l.resets_at),
                claude_fallback(claude),
            ),
        },
        Metric {
            title: "CLAUDE · weekly".into(),
            short: "CL wk".into(),
            percent: cl
                .and_then(|l| l.secondary.as_ref())
                .map(|l| l.used_percent),
            note: reset_note(
                cl.and_then(|l| l.secondary.as_ref())
                    .and_then(|l| l.resets_at),
                String::new(),
            ),
        },
    ];

    let cx = codex.latest_limits.as_ref();
    let codex_metrics = vec![
        Metric {
            title: "CODEX · 5h".into(),
            short: "CX 5h".into(),
            percent: cx.and_then(|l| l.primary.as_ref()).map(|l| l.used_percent),
            note: reset_note(
                cx.and_then(|l| l.primary.as_ref())
                    .and_then(|l| l.resets_at),
                String::new(),
            ),
        },
        Metric {
            title: "CODEX · weekly".into(),
            short: "CX wk".into(),
            percent: cx
                .and_then(|l| l.secondary.as_ref())
                .map(|l| l.used_percent),
            note: reset_note(
                cx.and_then(|l| l.secondary.as_ref())
                    .and_then(|l| l.resets_at),
                String::new(),
            ),
        },
    ];

    (claude_metrics, codex_metrics)
}

fn collect_context(codex: &CodexImportCache) -> Option<Context> {
    let snap = codex_session_snapshot(codex)?;
    let tokens = snap.latest_input.saturating_sub(snap.latest_cached) + snap.latest_output;
    let window = snap.latest_context_window;
    if window == 0 {
        return None;
    }
    Some(Context {
        percent: tokens as f64 / window as f64 * 100.0,
        detail: format!("{}K / {}K", tokens / 1000, window / 1000),
    })
}

fn claude_fallback(claude: &ClaudeImportDiagnostics) -> String {
    match &claude.fetch_error {
        Some(_) => "no auth".into(),
        None => String::new(),
    }
}

fn reset_note(resets_at: Option<u64>, fallback: String) -> String {
    match resets_at {
        Some(_) => format!("resets {}", format_reset(resets_at)),
        None => fallback,
    }
}

/// Top-level responsive layout. Wide terminals get two odometer columns plus a
/// context bar; narrow ones stack a single column.
fn render_dashboard(
    frame: &mut Frame<'_>,
    area: Rect,
    claude: &[Metric],
    codex: &[Metric],
    context: Option<&Context>,
) {
    // Reserve the last row for the full-width context bar when present.
    let (grid, ctx_area) = if context.is_some() && area.height >= 4 {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);
        (parts[0], Some(parts[1]))
    } else {
        (area, None)
    };

    let two_col = grid.width >= 56;
    if two_col {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(grid);
        render_metric_column(frame, cols[0], claude);
        render_metric_column(frame, cols[1], codex);
    } else {
        // Single column: interleave so 5h limits sit above weekly limits.
        let merged: Vec<&Metric> = claude.iter().chain(codex.iter()).collect();
        render_metric_column_refs(frame, grid, &merged);
    }

    if let (Some(ctx_area), Some(ctx)) = (ctx_area, context) {
        render_context_bar(frame, ctx_area, ctx);
    }
}

fn render_metric_column(frame: &mut Frame<'_>, area: Rect, metrics: &[Metric]) {
    let refs: Vec<&Metric> = metrics.iter().collect();
    render_metric_column_refs(frame, area, &refs);
}

fn render_metric_column_refs(frame: &mut Frame<'_>, area: Rect, metrics: &[&Metric]) {
    if metrics.is_empty() || area.height == 0 {
        return;
    }

    // A tall drum needs 3 rows; fall back to compact single-row rows otherwise.
    let tall = metrics.len() * 3 <= area.height as usize && area.width >= 28;
    let cell_h = if tall { 3usize } else { 1usize };

    // Center the stack vertically, spreading any leftover rows as even gaps
    // between metrics.
    let used = metrics.len() * cell_h;
    let slack = (area.height as usize).saturating_sub(used);
    let gap = slack / (metrics.len() + 1);
    let top = (slack - gap * metrics.len()) / 2;

    for (i, metric) in metrics.iter().enumerate() {
        let y = area.y + (top + i * (cell_h + gap)) as u16;
        if y >= area.y + area.height {
            break;
        }
        let h = (cell_h as u16).min(area.y + area.height - y);
        let cell = Rect {
            x: area.x,
            y,
            width: area.width,
            height: h,
        };
        if tall && h >= 3 {
            render_tall_metric(frame, cell, metric);
        } else {
            render_compact_metric(frame, cell, metric);
        }
    }
}

/// Tall: 3-row boxed-digit drum on the left, title + bar + reset on the right.
fn render_tall_metric(frame: &mut Frame<'_>, area: Rect, metric: &Metric) {
    let color = metric.percent.map(percent_color).unwrap_or(Color::DarkGray);
    let digits = digit_string(metric.percent);
    let (top, mid, bot) = drum_lines(&digits);
    let drum_w = top.chars().count() as u16;

    let drum = Rect {
        x: area.x + 2,
        y: area.y,
        width: drum_w.min(area.width.saturating_sub(2)),
        height: 3,
    };
    let drum_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(top, drum_style),
            Line::styled(mid, drum_style),
            Line::styled(bot, drum_style),
        ]),
        drum,
    );

    let rx = drum.x + drum.width + 2;
    if rx >= area.x + area.width {
        return;
    }
    let rw = area.x + area.width - rx;
    let right = Rect {
        x: rx,
        y: area.y,
        width: rw,
        height: 3,
    };

    let bar_w = rw.saturating_sub(1) as usize;
    let mut lines = vec![Line::styled(
        truncate(&metric.title, rw as usize),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )];
    lines.push(if bar_w >= 3 {
        Line::from(bar_span(metric.percent.unwrap_or(0.0), bar_w, color))
    } else {
        Line::raw("")
    });
    lines.push(Line::styled(
        truncate(&metric.note, rw as usize),
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(lines), right);
}

/// Compact: ` TITLE [NN%] ████░░░ note ` on a single line.
fn render_compact_metric(frame: &mut Frame<'_>, area: Rect, metric: &Metric) {
    let color = metric.percent.map(percent_color).unwrap_or(Color::DarkGray);
    let pct = match metric.percent {
        Some(p) => format!("{:>3.0}%", p.min(999.0)),
        None => "  --".into(),
    };
    let title = truncate(&metric.short, 6);
    let title_pad = format!("{title:<6}");

    let avail = area.width as usize;
    let fixed = 1 + title_pad.chars().count() + 1 + pct.len() + 2; // leading space + [pct] + space
    let note = if is_blank(&metric.note) {
        String::new()
    } else {
        metric.note.clone()
    };
    let note_w = note.chars().count().min(avail.saturating_sub(fixed + 4));
    let bar_w = avail.saturating_sub(fixed + note_w + 1);

    let mut spans = vec![
        Span::styled(format!(" {title_pad}"), Style::default().fg(Color::White)),
        Span::styled(
            format!("[{pct}]"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ];
    if bar_w >= 3 {
        spans.push(Span::raw(" "));
        spans.push(bar_span(metric.percent.unwrap_or(0.0), bar_w, color));
    }
    if note_w > 0 {
        spans.push(Span::styled(
            format!(" {}", truncate(&note, note_w)),
            Style::default().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_context_bar(frame: &mut Frame<'_>, area: Rect, ctx: &Context) {
    let color = percent_color(ctx.percent);
    let label = " CONTEXT ";
    let pct = format!("{:>3.0}%", ctx.percent.min(999.0));
    let detail = format!("  {}", ctx.detail);

    let avail = area.width as usize;
    let fixed = label.len() + 1 + pct.len() + detail.chars().count();
    let bar_w = avail.saturating_sub(fixed + 1);

    let mut spans = vec![Span::styled(
        label,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )];
    if bar_w >= 3 {
        spans.push(bar_span(ctx.percent, bar_w, color));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        pct,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(detail, Style::default().fg(Color::DarkGray)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Three text rows of a boxed-digit odometer drum, with a trailing `%` cell.
fn drum_lines(digits: &str) -> (String, String, String) {
    let cells: Vec<char> = digits.chars().chain(std::iter::once('%')).collect();
    let n = cells.len();
    let mut top = String::from("┏");
    let mut mid = String::from("┃");
    let mut bot = String::from("┗");
    for (i, c) in cells.iter().enumerate() {
        top.push('━');
        mid.push(*c);
        bot.push('━');
        if i + 1 < n {
            top.push('┳');
            mid.push('┃');
            bot.push('┻');
        }
    }
    top.push('┓');
    mid.push('┃');
    bot.push('┛');
    (top, mid, bot)
}

/// Right-aligned 2-char percentage for the drum (e.g. "48", " 5", "--").
fn digit_string(percent: Option<f64>) -> String {
    match percent {
        Some(p) => format!("{:>2.0}", p.min(99.0)),
        None => "--".into(),
    }
}

/// A unicode block bar filled to `pct` percent, colored by threshold.
fn bar_span(pct: f64, width: usize, color: Color) -> Span<'static> {
    let ratio = (pct / 100.0).clamp(0.0, 1.0);
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let bar = format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    );
    Span::styled(bar, Style::default().fg(color))
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        String::new()
    } else if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

fn is_blank(s: &str) -> bool {
    s.is_empty() || s == "—"
}

fn percent_color(pct: f64) -> Color {
    if pct >= 90.0 {
        Color::Red
    } else if pct >= 70.0 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn format_reset(resets_at: Option<u64>) -> String {
    let Some(target) = resets_at else {
        return "—".into();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if target <= now {
        return "now".into();
    }
    let remaining = target - now;
    let d = remaining / 86400;
    let h = (remaining % 86400) / 3600;
    let m = (remaining % 3600) / 60;
    if d > 0 {
        format!("{d}d{h}h")
    } else if h > 0 {
        format!("{h}h{m}m")
    } else {
        format!("{m}m")
    }
}

fn draw_help_overlay(frame: &mut Frame<'_>) {
    let area = frame.area();
    let pct_x = if area.width < 40 { 90 } else { 50 };
    let pct_y = if area.height < 16 { 80 } else { 40 };
    let overlay = centered_rect(pct_x, pct_y, area);

    let help_lines = vec![
        Line::from("Controls"),
        Line::from("q : quit"),
        Line::from("r : reload"),
        Line::from("? : toggle help"),
    ];

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(help_lines).block(rounded_block(" Help ")),
        overlay,
    );
}

fn rounded_block<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .title(title)
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_import::{CodexRateLimit, CodexRateLimits};
    use crate::models::AppConfig;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn limits(p5: f64, pwk: f64) -> CodexRateLimits {
        let mk = |p: f64| CodexRateLimit {
            used_percent: p,
            resets_at: Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 3720,
            ),
        };
        CodexRateLimits {
            timestamp: "now".into(),
            primary: Some(mk(p5)),
            secondary: Some(mk(pwk)),
        }
    }

    fn sample_app() -> App {
        let mut claude_cache = ClaudeImportDiagnostics {
            five_hour_pct: 48.0,
            seven_day_pct: 8.0,
            limits: Some(limits(48.0, 8.0)),
            ..Default::default()
        };
        claude_cache.fetch_error = None;

        // input - cached + output = 65016, window 258400 -> ~25%.
        let mut codex_cache = CodexImportCache::with_test_context(384455, 326656, 7217, 258400);
        codex_cache.latest_limits = Some(limits(5.0, 32.0));

        App {
            config: AppConfig::default(),
            codex_cache,
            claude_cache,
            show_help: false,
        }
    }

    fn render_at(width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let app = sample_app();
        terminal.draw(|f| draw(f, &app)).expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn rows_match_terminal_width_at_all_sizes() {
        for (w, h) in [(60, 20), (48, 16), (40, 12), (80, 24), (120, 40), (100, 30)] {
            let rows = render_at(w, h);
            assert_eq!(rows.len(), h as usize);
            for (i, row) in rows.iter().enumerate() {
                assert_eq!(
                    row.chars().count(),
                    w as usize,
                    "size {w}x{h} row {i} overflowed: {row:?}"
                );
            }
        }
    }

    #[test]
    fn target_60x20_shows_drums_and_context() {
        let joined = render_at(60, 20).join("\n");
        assert!(joined.contains("PromptPetrol"));
        assert!(
            joined.contains('┏') && joined.contains('┛'),
            "drums:\n{joined}"
        );
        assert!(joined.contains("CLAUDE"), "claude:\n{joined}");
        assert!(joined.contains("CODEX"), "codex:\n{joined}");
        assert!(joined.contains("resets"), "reset note:\n{joined}");
    }

    #[test]
    fn drum_lines_aligned() {
        let (t, m, b) = drum_lines("48");
        assert_eq!(t.chars().count(), m.chars().count());
        assert_eq!(m.chars().count(), b.chars().count());
        assert!(t.starts_with('┏') && t.ends_with('┓'));
    }

    #[test]
    #[ignore = "visual inspection only"]
    fn dump_60x20() {
        for r in render_at(60, 20) {
            eprintln!("{r}");
        }
    }

    #[test]
    #[ignore = "visual inspection only"]
    fn dump_40x12() {
        for r in render_at(40, 12) {
            eprintln!("{r}");
        }
    }

    #[test]
    #[ignore = "visual inspection only"]
    fn dump_120x40() {
        for r in render_at(120, 40) {
            eprintln!("{r}");
        }
    }

    #[test]
    fn cluster_renders_at_120x40() {
        let joined = render_at(120, 40).join("\n");
        // Center context readout and labels present in the cluster layout.
        assert!(joined.contains("CONTEXT"), "context:\n{joined}");
        assert!(joined.contains("CLAUDE"), "claude:\n{joined}");
        assert!(joined.contains("CODEX"), "codex:\n{joined}");
    }
}

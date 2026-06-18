use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::claude_import::ClaudeImportDiagnostics;
use crate::codex_import::{CodexImportCache, codex_session_snapshot};

const APP_NAME: &str = "PromptPetrol";

pub(crate) fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let w = area.width;
    let h = area.height;

    if w < 12 || h < 6 {
        let msg = if w < 12 { "width" } else { "height" };
        frame.render_widget(
            Paragraph::new(format!("{APP_NAME} - terminal {msg} too small")),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_claude_panel(frame, chunks[0], &app.claude_cache);
    render_codex_panel(frame, chunks[1], &app.codex_cache);

    if app.show_help {
        draw_help_overlay(frame);
    }
}

fn render_claude_panel(frame: &mut Frame<'_>, area: Rect, cache: &ClaudeImportDiagnostics) {
    let inner = rounded_block("Claude").inner(area);
    frame.render_widget(rounded_block("Claude"), area);

    if inner.height < 4 || inner.width < 8 {
        return;
    }

    let limits = cache.limits.as_ref();

    let five_h_ratio = limits
        .and_then(|l| l.primary.as_ref())
        .map(|l| (l.used_percent / 100.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let seven_d_ratio = limits
        .and_then(|l| l.secondary.as_ref())
        .map(|l| (l.used_percent / 100.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    let gauges = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let min_g = 8u16;
    if gauges[0].width >= min_g && gauges[0].height >= 6 {
        render_gauge(frame, gauges[0], "5h Limit", five_h_ratio);
    } else {
        render_bar(frame, gauges[0], "5h", five_h_ratio);
    }
    if gauges[1].width >= min_g && gauges[1].height >= 6 {
        render_gauge(frame, gauges[1], "7d Limit", seven_d_ratio);
    } else {
        render_bar(frame, gauges[1], "7d", seven_d_ratio);
    }

    let text_y = inner.y + inner.height.saturating_sub(5);
    let text_h = 5.min(inner.height);
    let text_area = Rect {
        x: inner.x,
        y: text_y,
        width: inner.width,
        height: text_h,
    };

    let mut lines = Vec::new();

    // The OAuth usage endpoint reports utilization percentages directly; prefer
    // the parsed limits, falling back to the diagnostics percent fields.
    let five_h_pct = limits
        .and_then(|l| l.primary.as_ref())
        .map(|l| l.used_percent)
        .unwrap_or(cache.five_hour_pct);
    let seven_d_pct = limits
        .and_then(|l| l.secondary.as_ref())
        .map(|l| l.used_percent)
        .unwrap_or(cache.seven_day_pct);

    lines.push(Line::from(vec![
        Span::styled(" 5h Limit ", Style::default().fg(Color::Cyan)),
        Span::styled(
            format!("{five_h_pct:>5.1}%"),
            Style::default().fg(percent_color(five_h_pct)),
        ),
    ]));

    let five_h_reset = limits
        .and_then(|l| l.primary.as_ref())
        .map(|l| format_reset(l.resets_at))
        .unwrap_or_else(|| "unknown".into());
    lines.push(Line::from(vec![Span::styled(
        format!("      reset {five_h_reset}"),
        Style::default().fg(Color::DarkGray),
    )]));

    lines.push(Line::from(vec![
        Span::styled(" Weekly   ", Style::default().fg(Color::Cyan)),
        Span::styled(
            format!("{seven_d_pct:>5.1}%"),
            Style::default().fg(percent_color(seven_d_pct)),
        ),
    ]));

    let seven_d_reset = limits
        .and_then(|l| l.secondary.as_ref())
        .map(|l| format_reset(l.resets_at))
        .unwrap_or_else(|| "unknown".into());
    lines.push(Line::from(vec![Span::styled(
        format!("      reset {seven_d_reset}"),
        Style::default().fg(Color::DarkGray),
    )]));

    frame.render_widget(Paragraph::new(lines), text_area);
}

fn render_codex_panel(frame: &mut Frame<'_>, area: Rect, cache: &CodexImportCache) {
    let inner = rounded_block("Codex").inner(area);
    frame.render_widget(rounded_block("Codex"), area);

    if inner.height < 4 || inner.width < 8 {
        return;
    }

    let limits = cache.latest_limits.as_ref();

    let five_h_ratio = limits
        .and_then(|l| l.primary.as_ref())
        .map(|l| (l.used_percent / 100.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let weekly_ratio = limits
        .and_then(|l| l.secondary.as_ref())
        .map(|l| (l.used_percent / 100.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    let gauges = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let min_g = 8u16;
    if gauges[0].width >= min_g && gauges[0].height >= 6 {
        render_gauge(frame, gauges[0], "5h Limit", five_h_ratio);
    } else {
        render_bar(frame, gauges[0], "5h", five_h_ratio);
    }
    if gauges[1].width >= min_g && gauges[1].height >= 6 {
        render_gauge(frame, gauges[1], "Weekly", weekly_ratio);
    } else {
        render_bar(frame, gauges[1], "WK", weekly_ratio);
    }

    let text_y = inner.y + inner.height.saturating_sub(5);
    let text_h = 5.min(inner.height);
    let text_area = Rect {
        x: inner.x,
        y: text_y,
        width: inner.width,
        height: text_h,
    };

    let mut lines = Vec::new();

    let five_h_used = limits
        .and_then(|l| l.primary.as_ref())
        .map(|l| format!("{:.1}%", l.used_percent))
        .unwrap_or_else(|| "--".into());
    let weekly_used = limits
        .and_then(|l| l.secondary.as_ref())
        .map(|l| format!("{:.1}%", l.used_percent))
        .unwrap_or_else(|| "--".into());

    let five_h_color = limits
        .and_then(|l| l.primary.as_ref())
        .map(|l| percent_color(l.used_percent))
        .unwrap_or(Color::DarkGray);
    let weekly_color = limits
        .and_then(|l| l.secondary.as_ref())
        .map(|l| percent_color(l.used_percent))
        .unwrap_or(Color::DarkGray);

    lines.push(Line::from(vec![
        Span::styled(
            format!(" 5h     {five_h_used:>6}"),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            format!(" {} ", color_label(five_h_color)),
            Style::default()
                .fg(Color::Black)
                .bg(five_h_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    let five_h_reset = limits
        .and_then(|l| l.primary.as_ref())
        .map(|l| format_reset(l.resets_at))
        .unwrap_or_else(|| "unknown".into());
    lines.push(Line::from(vec![Span::styled(
        format!("         reset {five_h_reset}"),
        Style::default().fg(Color::DarkGray),
    )]));

    lines.push(Line::from(vec![
        Span::styled(
            format!(" Wkly   {weekly_used:>6}"),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            format!(" {} ", color_label(weekly_color)),
            Style::default()
                .fg(Color::Black)
                .bg(weekly_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    let weekly_reset = limits
        .and_then(|l| l.secondary.as_ref())
        .map(|l| format_reset(l.resets_at))
        .unwrap_or_else(|| "unknown".into());
    lines.push(Line::from(vec![Span::styled(
        format!("         reset {weekly_reset}"),
        Style::default().fg(Color::DarkGray),
    )]));

    // Show context window from the most-recent session (a per-conversation
    // measure), not the lifetime sum across all sessions.
    if let Some(snap) = codex_session_snapshot(cache) {
        // Codex reports input_tokens cumulatively (cache re-reads included), so
        // it overshoots the window. Live residency = fresh input + output.
        let ctx_tokens = snap.latest_input.saturating_sub(snap.latest_cached) + snap.latest_output;
        let ctx_window = snap.latest_context_window;

        let age_str = match snap.limits_age_secs {
            Some(age) if age < 300 => "live".to_string(),
            Some(age) if age < 3600 => format!("{}m ago", age / 60),
            Some(age) if age < 86400 => format!("{}h ago", age / 3600),
            Some(age) => format!("{}d ago", age / 86400),
            None => "unknown".to_string(),
        };
        lines.push(Line::from(vec![Span::styled(
            format!(" Updated {age_str}"),
            Style::default().fg(Color::DarkGray),
        )]));

        if ctx_window > 0 && ctx_tokens > 0 {
            let ctx_pct = ctx_tokens as f64 / ctx_window as f64 * 100.0;
            let ctx_color = if ctx_pct >= 90.0 {
                Color::Red
            } else if ctx_pct >= 70.0 {
                Color::Yellow
            } else {
                Color::Green
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" CTX {:.0}%", ctx_pct),
                    Style::default().fg(ctx_color),
                ),
                Span::styled(
                    format!(" {}K/{}K", ctx_tokens / 1000, ctx_window / 1000),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(lines), text_area);
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

fn color_label(c: Color) -> &'static str {
    match c {
        Color::Red => "ALERT",
        Color::Yellow => "WATCH",
        _ => "NOM",
    }
}

fn render_gauge(frame: &mut Frame<'_>, area: Rect, title: &str, ratio: f64) {
    let ratio = ratio.clamp(0.0, 1.0);
    let gauge_color = if ratio >= 0.9 {
        Color::Red
    } else if ratio >= 0.7 {
        Color::Yellow
    } else {
        Color::Cyan
    };

    frame.render_widget(
        Canvas::default()
            .block(rounded_block(title))
            .x_bounds([-1.2, 1.2])
            .y_bounds([-1.2, 1.2])
            .paint(|ctx| {
                ctx.draw(&Circle {
                    x: 0.0,
                    y: 0.0,
                    radius: 1.0,
                    color: Color::DarkGray,
                });

                let tick_count = if area.width < 14 || area.height < 10 {
                    5
                } else {
                    10
                };
                for step in 0..=tick_count {
                    let t = step as f64 / tick_count as f64;
                    let angle = (225.0 - 270.0 * t).to_radians();
                    ctx.draw(&CanvasLine {
                        x1: angle.cos() * 0.82,
                        y1: angle.sin() * 0.82,
                        x2: angle.cos() * 0.96,
                        y2: angle.sin() * 0.96,
                        color: Color::Gray,
                    });
                }

                let angle = (225.0 - 270.0 * ratio).to_radians();
                ctx.draw(&CanvasLine {
                    x1: 0.0,
                    y1: 0.0,
                    x2: angle.cos() * 0.76,
                    y2: angle.sin() * 0.76,
                    color: gauge_color,
                });
                ctx.draw(&Circle {
                    x: 0.0,
                    y: 0.0,
                    radius: 0.05,
                    color: Color::White,
                });
            }),
        area,
    );

    let value_text = if area.width >= 14 {
        format!("{:>5.1}%", ratio * 100.0)
    } else if area.width >= 8 {
        format!("{:>4.0}%", ratio * 100.0)
    } else {
        return;
    };
    let value_area = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(area.height.saturating_sub(2)),
        width: area.width.saturating_sub(2),
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(value_text).style(
            Style::default()
                .fg(gauge_color)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        value_area,
    );
}

fn render_bar(frame: &mut Frame<'_>, area: Rect, label: &str, ratio: f64) {
    let ratio = ratio.clamp(0.0, 1.0);
    let bar_color = percent_color(ratio * 100.0);

    let block = rounded_block(label);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 6 || inner.height < 1 {
        return;
    }

    let pct_str = format!("{:.0}%", ratio * 100.0);
    let bar_chars = inner.width.saturating_sub(pct_str.len() as u16 + 3) as usize;
    if bar_chars < 2 {
        return;
    }

    let filled = ((ratio * bar_chars as f64).round() as usize).min(bar_chars);
    let empty = bar_chars - filled;

    let bar_row = format!("[{}{}] {pct_str}", "#".repeat(filled), "-".repeat(empty),);
    frame.render_widget(
        Paragraph::new(bar_row).style(Style::default().fg(bar_color)),
        inner,
    );
}

fn format_reset(resets_at: Option<u64>) -> String {
    let Some(target) = resets_at else {
        return "unknown".into();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if target <= now {
        return "now".into();
    }
    let remaining = target - now;
    let h = remaining / 3600;
    let m = (remaining % 3600) / 60;
    if h > 0 {
        format!("in {h}h{m}m")
    } else {
        format!("in {m}m")
    }
}

fn draw_help_overlay(frame: &mut Frame<'_>) {
    let area = frame.area();
    let pct_x = if area.width < 30 { 80 } else { 50 };
    let pct_y = if area.height < 15 { 70 } else { 40 };
    let overlay = centered_rect(pct_x, pct_y, area);

    let help_lines = vec![
        Line::from("Controls"),
        Line::from("q : quit"),
        Line::from("r : reload"),
        Line::from("? : toggle help"),
    ];

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(help_lines).block(rounded_block("Help")),
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

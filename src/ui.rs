use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Context as CanvasContext, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::claude_import::ClaudeImportDiagnostics;
use crate::codex_import::{CodexImportCache, codex_session_snapshot};
use crate::models::Theme;

// Decorative motion uses monotonic time, independent of provider refreshes.
fn animation_seconds() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

fn activity_strip(seconds: f64) -> String {
    let head = (seconds * 8.0) as usize % 12;
    (0..12)
        .map(|i| match (head + 12 - i) % 12 {
            0 => '▰',
            1 | 2 => '▪',
            _ => '·',
        })
        .collect()
}

#[derive(Clone, Copy)]
struct Palette {
    background: Color,
    surface: Color,
    normal: Color,
    separator: Color,
    accent: Color,
    label: Color,
    warning: Color,
    decorative: Color,
    text: Color,
    muted: Color,
    dim: Color,
    critical: Color,
}

const CARBON: Palette = Palette {
    background: Color::Rgb(12, 16, 23),
    surface: Color::Rgb(19, 26, 36),
    normal: Color::Rgb(112, 224, 190),
    separator: Color::Rgb(43, 56, 73),
    accent: Color::Rgb(124, 196, 255),
    label: Color::Rgb(226, 193, 139),
    warning: Color::Rgb(244, 188, 111),
    decorative: Color::Rgb(183, 166, 242),
    text: Color::Rgb(230, 237, 245),
    muted: Color::Rgb(148, 163, 184),
    dim: Color::Rgb(85, 104, 127),
    critical: Color::Rgb(255, 119, 137),
};

// Neovim's built-in Murphy palette, resolved from the active highlight groups.
const MURPHY: Palette = Palette {
    background: Color::Rgb(0, 0, 0),
    surface: Color::Rgb(8, 16, 10),
    normal: Color::Rgb(135, 255, 135),
    separator: Color::Rgb(0, 95, 0),
    accent: Color::Rgb(0, 255, 255),
    label: Color::Rgb(255, 255, 0),
    warning: Color::Rgb(255, 167, 0),
    decorative: Color::Rgb(255, 0, 255),
    text: Color::Rgb(255, 255, 255),
    muted: Color::Rgb(188, 188, 188),
    dim: Color::Rgb(58, 58, 58),
    critical: Color::Rgb(255, 0, 0),
};

const DRACULA: Palette = Palette {
    background: Color::Rgb(40, 42, 54),
    surface: Color::Rgb(48, 51, 65),
    normal: Color::Rgb(80, 250, 123),
    separator: Color::Rgb(68, 71, 90),
    accent: Color::Rgb(139, 233, 253),
    label: Color::Rgb(241, 250, 140),
    warning: Color::Rgb(255, 184, 108),
    decorative: Color::Rgb(189, 147, 249),
    text: Color::Rgb(248, 248, 242),
    muted: Color::Rgb(184, 184, 196),
    dim: Color::Rgb(98, 114, 164),
    critical: Color::Rgb(255, 85, 85),
};

const NORD: Palette = Palette {
    background: Color::Rgb(46, 52, 64),
    surface: Color::Rgb(59, 66, 82),
    normal: Color::Rgb(163, 190, 140),
    separator: Color::Rgb(76, 86, 106),
    accent: Color::Rgb(136, 192, 208),
    label: Color::Rgb(235, 203, 139),
    warning: Color::Rgb(208, 135, 112),
    decorative: Color::Rgb(180, 142, 173),
    text: Color::Rgb(236, 239, 244),
    muted: Color::Rgb(216, 222, 233),
    dim: Color::Rgb(97, 110, 136),
    critical: Color::Rgb(191, 97, 106),
};

const GRUVBOX: Palette = Palette {
    background: Color::Rgb(40, 40, 40),
    surface: Color::Rgb(50, 48, 47),
    normal: Color::Rgb(184, 187, 38),
    separator: Color::Rgb(80, 73, 69),
    accent: Color::Rgb(131, 165, 152),
    label: Color::Rgb(250, 189, 47),
    warning: Color::Rgb(254, 128, 25),
    decorative: Color::Rgb(211, 134, 155),
    text: Color::Rgb(235, 219, 178),
    muted: Color::Rgb(189, 174, 147),
    dim: Color::Rgb(102, 92, 84),
    critical: Color::Rgb(251, 73, 52),
};

const TOKYO_NIGHT: Palette = Palette {
    background: Color::Rgb(26, 27, 38),
    surface: Color::Rgb(36, 40, 59),
    normal: Color::Rgb(158, 206, 106),
    separator: Color::Rgb(59, 66, 97),
    accent: Color::Rgb(125, 207, 255),
    label: Color::Rgb(224, 175, 104),
    warning: Color::Rgb(255, 158, 100),
    decorative: Color::Rgb(187, 154, 247),
    text: Color::Rgb(192, 202, 245),
    muted: Color::Rgb(121, 130, 169),
    dim: Color::Rgb(86, 95, 137),
    critical: Color::Rgb(247, 118, 142),
};

const PAPER: Palette = Palette {
    background: Color::Rgb(247, 243, 232),
    surface: Color::Rgb(255, 250, 240),
    normal: Color::Rgb(23, 107, 69),
    separator: Color::Rgb(177, 197, 185),
    accent: Color::Rgb(0, 100, 112),
    label: Color::Rgb(126, 91, 0),
    warning: Color::Rgb(163, 72, 0),
    decorative: Color::Rgb(126, 53, 99),
    text: Color::Rgb(34, 34, 34),
    muted: Color::Rgb(85, 89, 92),
    dim: Color::Rgb(134, 139, 136),
    critical: Color::Rgb(180, 35, 24),
};

const ARCTIC: Palette = Palette {
    background: Color::Rgb(243, 247, 251),
    surface: Color::Rgb(234, 241, 247),
    normal: Color::Rgb(11, 110, 79),
    separator: Color::Rgb(178, 199, 210),
    accent: Color::Rgb(0, 91, 145),
    label: Color::Rgb(117, 82, 0),
    warning: Color::Rgb(163, 74, 0),
    decorative: Color::Rgb(107, 55, 143),
    text: Color::Rgb(22, 33, 43),
    muted: Color::Rgb(76, 89, 101),
    dim: Color::Rgb(139, 153, 164),
    critical: Color::Rgb(176, 0, 32),
};

const SOLARIZED_LIGHT: Palette = Palette {
    background: Color::Rgb(253, 246, 227),
    surface: Color::Rgb(238, 232, 213),
    normal: Color::Rgb(92, 116, 0),
    separator: Color::Rgb(147, 161, 161),
    accent: Color::Rgb(25, 130, 122),
    label: Color::Rgb(154, 112, 0),
    warning: Color::Rgb(203, 75, 22),
    decorative: Color::Rgb(176, 47, 112),
    text: Color::Rgb(7, 54, 66),
    muted: Color::Rgb(88, 110, 117),
    dim: Color::Rgb(131, 148, 150),
    critical: Color::Rgb(220, 50, 47),
};

const fn palette_for(theme: Theme) -> Palette {
    match theme {
        Theme::Carbon => CARBON,
        Theme::Murphy => MURPHY,
        Theme::Dracula => DRACULA,
        Theme::Nord => NORD,
        Theme::Gruvbox => GRUVBOX,
        Theme::TokyoNight => TOKYO_NIGHT,
        Theme::Paper => PAPER,
        Theme::Arctic => ARCTIC,
        Theme::SolarizedLight => SOLARIZED_LIGHT,
    }
}

/// One limit readout rendered as a boxed-digit odometer (drum + bar + reset).
struct Metric {
    /// Full title for the spacious (tall) layout, e.g. "CLAUDE · 5h".
    title: &'static str,
    /// Short title for the compact layout, e.g. "CL 5h".
    short: &'static str,
    /// `None` means the value is unavailable; the drum shows `--`.
    percent: Option<f64>,
    note: String,
}

/// The bottom context-window readout (a single full-width bar).
struct Context {
    percent: f64,
    detail: String,
    used_tokens: u64,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
}

pub(crate) fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let palette = palette_for(app.active_theme());
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.background).fg(palette.text)),
        area,
    );
    if area.width < 24 || area.height < 6 {
        frame.render_widget(
            Paragraph::new("PromptPetrol · enlarge terminal")
                .style(Style::default().fg(palette.muted)),
            area,
        );
        return;
    }
    let (state, color) = system_status(app, palette);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(palette.separator))
        .title(Line::from(vec![
            Span::styled(" ◈ PromptPetrol ", Style::default().fg(palette.text).bold()),
            Span::styled(
                " / INTELLIGENCE IN MOTION ",
                Style::default().fg(palette.dim),
            ),
        ]))
        .title_bottom(
            Line::styled(
                format!(" {} · {} ", system_icon(state), state),
                Style::default().fg(color),
            )
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let (claude, codex) = collect_metrics(&app.claude_cache, &app.codex_cache);
    let context = collect_context(&app.codex_cache);
    if inner.width >= 90 && inner.height >= 16 {
        render_cluster(
            frame,
            inner,
            app,
            &claude,
            &codex,
            context.as_ref(),
            palette,
        );
    } else {
        let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
        render_dashboard(frame, rows[0], &claude, &codex, context.as_ref(), palette);
        render_controls(frame, rows[1], palette);
    }
    if app.show_help {
        draw_help_overlay(frame, palette);
    }
}

fn panel(title: Line<'_>, palette: Palette) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(palette.separator))
        .style(Style::default().bg(palette.surface))
        .title(title)
}

fn render_cluster(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    claude: &[Metric],
    codex: &[Metric],
    context: Option<&Context>,
    palette: Palette,
) {
    let spacious = area.height >= 30;
    let rows = Layout::vertical([
        Constraint::Length(if spacious { 3 } else { 2 }),
        Constraint::Min(10),
        Constraint::Length(if spacious { 7 } else { 0 }),
        Constraint::Length(if spacious { 4 } else { 3 }),
    ])
    .split(area.inner(Margin::new(1, 0)));
    render_header(frame, rows[0], app, palette);
    let cols = Layout::horizontal([
        Constraint::Ratio(3, 10),
        Constraint::Length(1),
        Constraint::Ratio(4, 10),
        Constraint::Length(1),
        Constraint::Ratio(3, 10),
    ])
    .split(rows[1]);
    render_provider_bay(frame, cols[0], "CLAUDE", "ANTHROPIC", claude, palette);
    render_context_scope(frame, cols[2], context, palette);
    render_provider_bay(frame, cols[4], "CODEX", "OPENAI", codex, palette);
    if spacious {
        render_capacity_map(frame, rows[2].inner(Margin::new(0, 1)), context, palette);
    }
    render_footer(frame, rows[3], app, palette);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &App, palette: Palette) {
    let right = format!(
        "{}  {}   ◷ {}",
        activity_strip(animation_seconds()),
        app.active_theme().label(),
        chrono::Local::now().format("%H:%M")
    );
    let (left, gap, right) = fit_status_line(" USAGE / OVERVIEW", &right, area.width as usize);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(left, Style::default().fg(palette.text).bold()),
                Span::raw(gap),
                Span::styled(right, Style::default().fg(palette.muted)),
            ]),
            Line::styled(
                " Subscription limits & active context",
                Style::default().fg(palette.muted),
            ),
        ]),
        area,
    );
}

fn render_provider_bay(
    frame: &mut Frame<'_>,
    area: Rect,
    provider: &str,
    source: &str,
    metrics: &[Metric],
    palette: Palette,
) {
    let brand = if provider == "CLAUDE" {
        palette.label
    } else {
        palette.accent
    };
    let block = panel(
        Line::from(vec![
            Span::styled(
                format!(" {} {provider} ", provider_icon(provider)),
                Style::default().fg(brand).bold(),
            ),
            Span::styled(format!(" / {source} "), Style::default().fg(palette.dim)),
        ]),
        palette,
    );
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);
    let tall = inner.height >= 17;
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(if tall { 6 } else { 4 }),
    ])
    .split(inner);
    let online = metrics.iter().any(|m| m.percent.is_some());
    frame.render_widget(
        Paragraph::new(Line::styled(
            if online {
                "●  USAGE AVAILABLE"
            } else {
                "○  AWAITING DATA"
            },
            Style::default().fg(if online { palette.muted } else { palette.dim }),
        )),
        rows[0],
    );
    if let Some(metric) = metrics.first() {
        render_primary_gauge(frame, rows[1], metric, brand, palette);
    }
    if let Some(metric) = metrics.get(1) {
        render_weekly_meter(frame, rows[2], metric, brand, palette);
    }
}

fn metric_color(percent: Option<f64>, accent: Color, palette: Palette) -> Color {
    match percent {
        None => palette.dim,
        Some(value) if value >= 70.0 => percent_color(value, palette),
        Some(_) => accent,
    }
}

fn render_primary_gauge(
    frame: &mut Frame<'_>,
    area: Rect,
    metric: &Metric,
    brand: Color,
    palette: Palette,
) {
    let color = metric_color(metric.percent, brand, palette);
    let tall = area.height >= 10;
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(2),
        Constraint::Length(2),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(Line::styled("5H LIMIT", Style::default().fg(palette.muted)).centered()),
        rows[0],
    );
    if tall {
        frame.render_widget(
            Canvas::default()
                .background_color(palette.surface)
                .x_bounds([-1.35, 1.35])
                .y_bounds([-0.85, 1.3])
                .paint(|ctx| draw_dial(ctx, metric.percent, color, palette, false)),
            rows[1],
        );
        let readout = Rect::new(rows[1].x, rows[1].y + rows[1].height / 2, rows[1].width, 3);
        frame.render_widget(
            Paragraph::new(
                large_percent_lines(metric.percent)
                    .into_iter()
                    .map(|s| Line::styled(s, Style::default().fg(color).bold()).centered())
                    .collect::<Vec<_>>(),
            ),
            readout,
        );
    } else {
        let percent = metric
            .percent
            .map(|p| format!("{p:.0}%"))
            .unwrap_or_else(|| "--%".into());
        let mut lines = vec![Line::styled(percent, Style::default().fg(color).bold()).centered()];
        if rows[1].height >= 2 {
            lines.push(meter_line(
                metric.percent,
                rows[1].width as usize,
                color,
                palette,
            ));
        }
        frame.render_widget(Paragraph::new(lines), rows[1]);
    }
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(metric_state(metric.percent), Style::default().fg(color)).centered(),
            Line::styled(metric_note(metric), Style::default().fg(palette.muted)).centered(),
        ]),
        rows[2],
    );
}

fn render_weekly_meter(
    frame: &mut Frame<'_>,
    area: Rect,
    metric: &Metric,
    brand: Color,
    palette: Palette,
) {
    let color = metric_color(metric.percent, brand, palette);
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(palette.separator));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let value = metric
        .percent
        .map(|p| format!("{p:.0}%"))
        .unwrap_or_else(|| "--%".into());
    let (left, gap, right) = fit_status_line("7D LIMIT", &value, inner.width as usize);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(left, Style::default().fg(palette.muted)),
            Span::raw(gap),
            Span::styled(right, Style::default().fg(color).bold()),
        ]),
        meter_line(metric.percent, inner.width as usize, color, palette),
        Line::styled(metric_note(metric), Style::default().fg(palette.muted)),
    ];
    if inner.height >= 5 {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            match metric.percent {
                Some(p) => format!("◇  {:.0}% headroom", (100.0 - p).max(0.0)),
                None => "○  No weekly reading".into(),
            },
            Style::default().fg(palette.dim),
        ));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn metric_note(metric: &Metric) -> String {
    if metric.note.is_empty() {
        "reset unavailable".into()
    } else {
        metric.note.clone()
    }
}

/// A segmented meter uses separate colors for occupied and available capacity.
/// Unknown data deliberately has no filled cells.
fn meter_line(percent: Option<f64>, width: usize, color: Color, palette: Palette) -> Line<'static> {
    let filled = percent
        .map(|p| ((p.clamp(0.0, 100.0) / 100.0 * width as f64).round() as usize).min(width))
        .unwrap_or(0);
    Line::from(vec![
        Span::styled("━".repeat(filled), Style::default().fg(color)),
        Span::styled(
            "┄".repeat(width.saturating_sub(filled)),
            Style::default().fg(palette.separator),
        ),
    ])
}

/// The 270-degree instrument arc runs clockwise, leaving space for its readout.
fn draw_dial(
    ctx: &mut CanvasContext,
    percent: Option<f64>,
    color: Color,
    palette: Palette,
    ring: bool,
) {
    let ratio = percent.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0;
    let sweep = if ring { 360.0 } else { 270.0 };
    let start = if ring { 90.0 } else { 225.0 };
    let steps = 120;
    for i in 0..steps {
        let a = (start - i as f64 / steps as f64 * sweep).to_radians();
        let b = (start - (i + 1) as f64 / steps as f64 * sweep).to_radians();
        for radius in [0.92, 0.96, 1.0] {
            ctx.draw(&CanvasLine {
                x1: a.cos() * radius,
                y1: a.sin() * radius,
                x2: b.cos() * radius,
                y2: b.sin() * radius,
                color: if (i as f64 / steps as f64) < ratio {
                    color
                } else {
                    palette.separator
                },
            });
        }
    }
    for i in 0..=30 {
        let a = (start - i as f64 / 30.0 * sweep).to_radians();
        let major = i % 5 == 0;
        ctx.draw(&CanvasLine {
            x1: a.cos() * 1.06,
            y1: a.sin() * 1.06,
            x2: a.cos() * if major { 1.16 } else { 1.10 },
            y2: a.sin() * if major { 1.16 } else { 1.10 },
            color: if major { palette.muted } else { palette.dim },
        });
    }
    // A separate outer track carries a comet; the usage arc stays exact.
    let phase = animation_seconds() * 0.7;
    for i in 0..72 {
        let a = i as f64 / 72.0 * std::f64::consts::TAU;
        let b = (i + 1) as f64 / 72.0 * std::f64::consts::TAU;
        let tail = (phase - a).rem_euclid(std::f64::consts::TAU);
        ctx.draw(&CanvasLine {
            x1: a.cos() * 1.23,
            y1: a.sin() * 1.23,
            x2: b.cos() * 1.23,
            y2: b.sin() * 1.23,
            color: if tail < 0.15 {
                palette.text
            } else if tail < 0.5 {
                color
            } else if tail < 0.9 {
                palette.muted
            } else {
                palette.separator
            },
        });
    }
    // A small illuminated pointer marks the exact reading on the arc.
    if percent.is_some() {
        let a = (start - ratio * sweep).to_radians();
        ctx.draw(&CanvasLine {
            x1: a.cos() * 0.82,
            y1: a.sin() * 0.82,
            x2: a.cos() * 1.04,
            y2: a.sin() * 1.04,
            color: palette.text,
        });
    }
}

fn render_context_scope(
    frame: &mut Frame<'_>,
    area: Rect,
    context: Option<&Context>,
    palette: Palette,
) {
    let block = panel(
        Line::styled(
            " ◎ CONTEXT / ACTIVE SESSION ",
            Style::default().fg(palette.decorative).bold(),
        )
        .centered(),
        palette,
    );
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(3),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(
            Line::styled("CODEX CONTEXT WINDOW", Style::default().fg(palette.muted)).centered(),
        ),
        rows[0],
    );
    let percent = context.map(|c| c.percent);
    let color = metric_color(percent, palette.normal, palette);
    let tall = rows[1].height >= 10;
    frame.render_widget(
        Canvas::default()
            .background_color(palette.surface)
            .x_bounds([-1.45, 1.45])
            .y_bounds([-1.3, 1.3])
            .paint(|ctx| draw_dial(ctx, percent, color, palette, true)),
        rows[1],
    );
    let center_height = if tall { 5 } else { 2 };
    let center_width = if tall { 19 } else { 12 }.min(rows[1].width);
    let center = Rect::new(
        rows[1].x + rows[1].width.saturating_sub(center_width) / 2,
        rows[1].y + rows[1].height.saturating_sub(center_height) / 2,
        center_width,
        center_height.min(rows[1].height),
    );
    let mut lines = if tall {
        large_percent_lines(percent)
            .into_iter()
            .map(|s| Line::styled(s, Style::default().fg(color).bold()).centered())
            .collect::<Vec<_>>()
    } else {
        vec![
            Line::styled(
                percent
                    .map(|p| format!("{p:.0}%"))
                    .unwrap_or_else(|| "--%".into()),
                Style::default().fg(color).bold(),
            )
            .centered(),
        ]
    };
    if tall {
        lines.push(Line::raw(""));
    }
    lines.push(
        Line::styled(
            if context.is_some() {
                "CONTEXT USED"
            } else {
                "NO SESSION"
            },
            Style::default().fg(palette.muted),
        )
        .centered(),
    );
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(palette.surface)),
        center,
    );
    let lines = match context {
        Some(c) => vec![
            Line::styled(
                format!("{} tokens", c.detail),
                Style::default().fg(palette.text),
            )
            .centered(),
            meter_line(percent, rows[2].width as usize, color, palette),
            Line::styled(
                format!(
                    "◇ {}K tokens available",
                    c.total_tokens.saturating_sub(c.used_tokens) / 1000
                ),
                Style::default().fg(palette.muted),
            )
            .centered(),
        ],
        None => vec![
            Line::styled("○  No active telemetry", Style::default().fg(palette.muted)).centered(),
            Line::styled(
                "Open a Codex session to begin",
                Style::default().fg(palette.dim),
            )
            .centered(),
        ],
    };
    frame.render_widget(Paragraph::new(lines), rows[2]);
}

fn render_capacity_map(
    frame: &mut Frame<'_>,
    area: Rect,
    context: Option<&Context>,
    palette: Palette,
) {
    let block = panel(
        Line::styled(
            " ▤ CONTEXT CAPACITY ",
            Style::default().fg(palette.text).bold(),
        ),
        palette,
    );
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);
    let Some(c) = context else {
        frame.render_widget(
            Paragraph::new("○  Token composition will appear when a context reading is available.")
                .style(Style::default().fg(palette.dim)),
            inner,
        );
        return;
    };
    let width = inner.width as usize;
    // Scale over-capacity readings to their actual total without overflowing.
    let total = c.total_tokens.max(c.used_tokens).max(1) as f64;
    let input_width = (c.input_tokens as f64 / total * width as f64).round() as usize;
    let output_width = ((c.output_tokens as f64 / total * width as f64).round() as usize)
        .min(width.saturating_sub(input_width));
    let free_width = width.saturating_sub(input_width + output_width);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    format!("● Input {:.1}K   ", c.input_tokens as f64 / 1000.0),
                    Style::default().fg(palette.accent),
                ),
                Span::styled(
                    format!("● Output {:.1}K   ", c.output_tokens as f64 / 1000.0),
                    Style::default().fg(palette.decorative),
                ),
                Span::styled(
                    format!(
                        "○ Available {:.1}K",
                        c.total_tokens.saturating_sub(c.used_tokens) as f64 / 1000.0
                    ),
                    Style::default().fg(palette.muted),
                ),
            ]),
            Line::from(vec![
                Span::styled("━".repeat(input_width), Style::default().fg(palette.accent)),
                Span::styled(
                    "━".repeat(output_width),
                    Style::default().fg(palette.decorative),
                ),
                Span::styled(
                    "┄".repeat(free_width),
                    Style::default().fg(palette.separator),
                ),
            ]),
            Line::styled(
                "Latest session · input excludes cached tokens",
                Style::default().fg(palette.dim),
            ),
        ]),
        inner,
    );
}

fn provider_icon(provider: &str) -> &'static str {
    match provider {
        "CLAUDE" => "✳",
        "CODEX" => "◇",
        _ => "◆",
    }
}

fn system_icon(state: &str) -> &'static str {
    match state {
        "CONFIG FAULT" | "REFRESH FAULT" => "×",
        "DEGRADED" => "△",
        "SYNCING" => "◌",
        _ => "●",
    }
}

fn system_status(app: &App, palette: Palette) -> (&'static str, Color) {
    let d = &app.codex_cache.diagnostics;
    if app.config_error.is_some() {
        ("CONFIG FAULT", palette.critical)
    } else if app.refresh_error.is_some() {
        ("REFRESH FAULT", palette.critical)
    } else if d.discovery_error.is_some()
        || d.parse_error_files > 0
        || d.unreadable_files > 0
        || (app.config.claude_import.enabled && app.claude_cache.fetch_error.is_some())
    {
        ("DEGRADED", palette.warning)
    } else if app.is_refreshing() {
        ("SYNCING", palette.accent)
    } else {
        ("READY", palette.normal)
    }
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App, palette: Palette) {
    let d = &app.codex_cache.diagnostics;
    let (state, color) = system_status(app, palette);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);
    let errors =
        d.parse_error_files + d.unreadable_files + usize::from(d.discovery_error.is_some());
    let status = format!(
        "  Codex {} files · age {} · {}ms   /   Claude age {} · {}ms   /   {} issues",
        d.active_files,
        source_age(d.last_success_at, d.last_attempt_at),
        duration_millis(d.last_duration),
        source_age(
            app.claude_cache.last_success_at,
            app.claude_cache.last_attempt_at
        ),
        duration_millis(app.claude_cache.last_duration),
        errors
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(
                    "{} {state}",
                    if state == "SYNCING" {
                        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
                            [(animation_seconds() * 10.0) as usize % 10]
                    } else {
                        system_icon(state)
                    }
                ),
                Style::default().fg(color),
            ),
            Span::styled(status, Style::default().fg(palette.muted)),
        ])),
        rows[0],
    );
    if area.height >= 4 {
        frame.render_widget(
            Paragraph::new(format!(
                "  Discovery {}s  ·  {} refreshed  ·  {} without usage",
                d.discovery_interval.as_secs(),
                d.refreshed_files,
                d.no_usage_or_limits_files
            ))
            .style(Style::default().fg(palette.dim)),
            rows[1],
        );
    }
    render_controls(frame, rows[2], palette);
}

fn render_controls(frame: &mut Frame<'_>, area: Rect, palette: Palette) {
    if area.width < 46 {
        frame.render_widget(
            Paragraph::new("r ↻ t ◈ ? help q quit").style(Style::default().fg(palette.muted)),
            area,
        );
        return;
    }
    let mut spans = Vec::new();
    for (key, label) in [
        ("r", "refresh"),
        ("t", "theme"),
        ("?", "help"),
        ("q", "quit"),
    ] {
        spans.push(Span::styled(
            format!(" {key} "),
            Style::default()
                .fg(palette.accent)
                .bg(palette.surface)
                .bold(),
        ));
        spans.push(Span::styled(
            format!(" {label}  "),
            Style::default().fg(palette.muted),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn metric_state(percent: Option<f64>) -> &'static str {
    match percent {
        None => "NO DATA",
        Some(value) if value >= 90.0 => "CRITICAL",
        Some(value) if value >= 70.0 => "CAUTION",
        Some(_) => "NOMINAL",
    }
}

fn large_percent_lines(percent: Option<f64>) -> [String; 3] {
    let value = percent
        .map(|value| format!("{:.0}%", value.clamp(0.0, 999.0)))
        .unwrap_or_else(|| "--%".into());
    let mut lines = [String::new(), String::new(), String::new()];
    for character in value.chars() {
        let glyph = large_glyph(character);
        for row in 0..3 {
            if !lines[row].is_empty() {
                lines[row].push(' ');
            }
            lines[row].push_str(glyph[row]);
        }
    }
    lines
}

fn large_glyph(character: char) -> [&'static str; 3] {
    match character {
        '0' => ["┌─┐", "│ │", "└─┘"],
        '1' => [" ╷ ", " │ ", " ╵ "],
        '2' => ["╶─┐", "┌─┘", "└─╴"],
        '3' => ["╶─┐", " ╶┤", "╶─┘"],
        '4' => ["╷ ╷", "└─┤", "  ╵"],
        '5' => ["┌─╴", "└─┐", "╶─┘"],
        '6' => ["┌─╴", "├─┐", "└─┘"],
        '7' => ["╶─┐", "  │", "  ╵"],
        '8' => ["┌─┐", "├─┤", "└─┘"],
        '9' => ["┌─┐", "└─┤", "╶─┘"],
        '%' => ["╷ ╷", " ╱ ", "╵ ╵"],
        _ => ["   ", "───", "   "],
    }
}

fn fit_status_line(left: &str, right: &str, width: usize) -> (String, String, String) {
    let left = truncate(left, width.saturating_sub(right.chars().count() + 1));
    let gap = width.saturating_sub(left.chars().count() + right.chars().count());
    (left, " ".repeat(gap), truncate(right, width))
}

fn collect_metrics(
    claude: &ClaudeImportDiagnostics,
    codex: &CodexImportCache,
) -> ([Metric; 2], [Metric; 2]) {
    let cl = claude.limits.as_ref();
    let claude_metrics = [
        Metric {
            title: "CLAUDE · 5h",
            short: "CL 5h",
            percent: cl.and_then(|l| l.primary.as_ref()).map(|l| l.used_percent),
            note: reset_note(
                cl.and_then(|l| l.primary.as_ref())
                    .and_then(|l| l.resets_at),
                claude_fallback(claude),
            ),
        },
        Metric {
            title: "CLAUDE · weekly",
            short: "CL wk",
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
    let codex_metrics = [
        Metric {
            title: "CODEX · 5h",
            short: "CX 5h",
            percent: cx.and_then(|l| l.primary.as_ref()).map(|l| l.used_percent),
            note: reset_note(
                cx.and_then(|l| l.primary.as_ref())
                    .and_then(|l| l.resets_at),
                String::new(),
            ),
        },
        Metric {
            title: "CODEX · weekly",
            short: "CX wk",
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
    let tokens = snap.latest_input.saturating_add(snap.latest_output);
    let window = snap.latest_context_window;
    if window == 0 {
        return None;
    }
    Some(Context {
        percent: tokens as f64 / window as f64 * 100.0,
        detail: format!("{}K / {}K", tokens / 1000, window / 1000),
        used_tokens: tokens,
        total_tokens: window,
        input_tokens: snap.latest_input,
        output_tokens: snap.latest_output,
    })
}

fn claude_fallback(claude: &ClaudeImportDiagnostics) -> String {
    match &claude.fetch_error {
        Some(error) if error == "Disabled" => "disabled".into(),
        Some(error) if error.starts_with("No OAuth token") => "no auth".into(),
        Some(error) if error.starts_with("Auth failed") => "auth failed".into(),
        Some(_) => "fetch error".into(),
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
    palette: Palette,
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
        for (column, provider, metrics) in [(cols[0], "CLAUDE", claude), (cols[1], "CODEX", codex)]
        {
            let block = panel(
                Line::styled(
                    format!(" {} {provider} ", provider_icon(provider)),
                    Style::default()
                        .fg(if provider == "CLAUDE" {
                            palette.label
                        } else {
                            palette.accent
                        })
                        .bold(),
                ),
                palette,
            );
            let inner = block.inner(column);
            frame.render_widget(block, column);
            render_metric_column(frame, inner, metrics, palette);
        }
    } else {
        // Single column: interleave so 5h limits sit above weekly limits.
        let merged: Vec<&Metric> = claude.iter().chain(codex.iter()).collect();
        render_metric_column_refs(frame, grid, &merged, palette);
    }

    if let (Some(ctx_area), Some(ctx)) = (ctx_area, context) {
        render_context_bar(frame, ctx_area, ctx, palette);
    }
}

fn render_metric_column(frame: &mut Frame<'_>, area: Rect, metrics: &[Metric], palette: Palette) {
    let refs: Vec<&Metric> = metrics.iter().collect();
    render_metric_column_refs(frame, area, &refs, palette);
}

fn render_metric_column_refs(
    frame: &mut Frame<'_>,
    area: Rect,
    metrics: &[&Metric],
    palette: Palette,
) {
    if metrics.is_empty() || area.height == 0 {
        return;
    }

    // A tall drum needs 3 rows; fall back to compact single-row rows otherwise.
    let tall = metrics.len() * 3 <= area.height as usize && area.width >= 24;
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
            render_tall_metric(frame, cell, metric, palette);
        } else {
            render_compact_metric(frame, cell, metric, palette);
        }
    }
}

/// Tall: 3-row boxed-digit drum on the left, title + bar + reset on the right.
fn render_tall_metric(frame: &mut Frame<'_>, area: Rect, metric: &Metric, palette: Palette) {
    let color = metric
        .percent
        .map(|percent| percent_color(percent, palette))
        .unwrap_or(palette.dim);
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
        truncate(metric.title, rw as usize),
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD),
    )];
    lines.push(if bar_w >= 3 {
        Line::from(bar_span(metric.percent.unwrap_or(0.0), bar_w, color))
    } else {
        Line::raw("")
    });
    lines.push(Line::styled(
        truncate(&metric.note, rw as usize),
        Style::default().fg(palette.warning),
    ));
    frame.render_widget(Paragraph::new(lines), right);
}

/// Compact: ` TITLE [NN%] ████░░░ note ` on a single line.
fn render_compact_metric(frame: &mut Frame<'_>, area: Rect, metric: &Metric, palette: Palette) {
    let color = metric
        .percent
        .map(|percent| percent_color(percent, palette))
        .unwrap_or(palette.dim);
    let pct = match metric.percent {
        Some(p) => format!("{:>3.0}%", p.min(999.0)),
        None => "  --".into(),
    };
    let title = truncate(metric.short, 6);
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
        Span::styled(format!(" {title_pad}"), Style::default().fg(palette.text)),
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
            Style::default().fg(palette.warning),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_context_bar(frame: &mut Frame<'_>, area: Rect, ctx: &Context, palette: Palette) {
    let color = percent_color(ctx.percent, palette);
    let label = " CONTEXT ";
    let pct = format!("{:>3.0}%", ctx.percent.min(999.0));
    let detail = format!("  {}", ctx.detail);

    let avail = area.width as usize;
    let fixed = label.len() + 1 + pct.len() + detail.chars().count();
    let bar_w = avail.saturating_sub(fixed + 1);

    let mut spans = vec![Span::styled(
        label,
        Style::default()
            .fg(palette.text)
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
    spans.push(Span::styled(detail, Style::default().fg(palette.warning)));
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
        Some(p) => format!("{:>2.0}", p.clamp(0.0, 999.0)),
        None => "--".into(),
    }
}

/// A unicode block bar filled to `pct` percent, colored by threshold.
fn bar_span(pct: f64, width: usize, color: Color) -> Span<'static> {
    let ratio = (pct / 100.0).clamp(0.0, 1.0);
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let bar = format!(
        "{}{}",
        "━".repeat(filled),
        "┄".repeat(width.saturating_sub(filled))
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

fn percent_color(pct: f64, palette: Palette) -> Color {
    if pct >= 90.0 {
        palette.critical
    } else if pct >= 70.0 {
        palette.warning
    } else {
        palette.normal
    }
}

fn source_age(success: Option<SystemTime>, attempt: Option<SystemTime>) -> String {
    success
        .or(attempt)
        .and_then(|time| time.elapsed().ok())
        .map(|age| format!("{}S", age.as_secs()))
        .unwrap_or_else(|| "--".into())
}

fn duration_millis(duration: Option<Duration>) -> String {
    duration
        .map(|duration| duration.as_millis().min(99_999).to_string())
        .unwrap_or_else(|| "--".into())
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

fn draw_help_overlay(frame: &mut Frame<'_>, palette: Palette) {
    let area = frame.area();
    let width = area.width.saturating_sub(2).min(62);
    let height = area.height.saturating_sub(2).min(13);
    let overlay = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let mut lines = vec![
        Line::styled(
            "KEYBOARD CONTROLS",
            Style::default().fg(palette.text).bold(),
        ),
        Line::raw(""),
    ];
    for (key, label) in [
        ("r", "Refresh data & reload config"),
        ("t", "Next color theme"),
        ("?", "Close this panel"),
        ("q", "Quit PromptPetrol"),
    ] {
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {key} "),
                Style::default()
                    .fg(palette.accent)
                    .bg(palette.background)
                    .bold(),
            ),
            Span::styled(format!("  {label}"), Style::default().fg(palette.text)),
        ]));
    }
    if height >= 13 {
        lines.extend([
            Line::raw(""),
            Line::styled(
                "● <70% normal   ◆ 70%+ caution   ▲ 90%+ critical",
                Style::default().fg(palette.muted),
            ),
            Line::styled(
                "Gauges show used capacity; -- means no reading.",
                Style::default().fg(palette.dim),
            ),
        ]);
    }
    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                panel(
                    Line::styled(
                        " ◇ QUICK REFERENCE ",
                        Style::default().fg(palette.decorative),
                    ),
                    palette,
                )
                .padding(ratatui::widgets::Padding::new(1, 1, 1, 0)),
            )
            .style(Style::default().bg(palette.surface)),
        overlay,
    );
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
        let mut claude_cache = ClaudeImportDiagnostics::default();
        claude_cache.five_hour_pct = 48.0;
        claude_cache.seven_day_pct = 8.0;
        claude_cache.limits = Some(limits(48.0, 8.0));
        claude_cache.fetch_error = None;

        // Cached input occupies context too: input + output = 65016.
        let mut codex_cache = CodexImportCache::with_test_context(57799, 32656, 7217, 258400);
        codex_cache.latest_limits = Some(limits(5.0, 32.0));

        App::with_test_state(AppConfig::default(), codex_cache, claude_cache)
    }

    fn sample_app_with_theme(theme: Theme) -> App {
        let mut app = sample_app();
        app.config.theme = theme;
        app
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
    fn every_theme_renders_its_palette_in_full_and_compact_layouts() {
        for theme in [
            Theme::Carbon,
            Theme::Murphy,
            Theme::Dracula,
            Theme::Nord,
            Theme::Gruvbox,
            Theme::TokyoNight,
            Theme::Paper,
            Theme::Arctic,
            Theme::SolarizedLight,
        ] {
            let palette = palette_for(theme);
            for (width, height) in [(120, 40), (60, 20)] {
                let backend = TestBackend::new(width, height);
                let mut terminal = Terminal::new(backend).expect("terminal");
                let app = sample_app_with_theme(theme);
                terminal.draw(|frame| draw(frame, &app)).expect("draw");
                let title_cell = &terminal.backend().buffer()[(0, 0)];

                assert_eq!(title_cell.bg, palette.background, "{theme:?} background");
                assert_eq!(title_cell.fg, palette.separator, "{theme:?} border");
            }
        }
    }

    #[test]
    fn rows_match_terminal_width_at_all_sizes() {
        for (w, h) in [
            (0, 0),
            (1, 1),
            (23, 5),
            (24, 6),
            (92, 18),
            (92, 32),
            (60, 20),
            (48, 16),
            (40, 12),
            (80, 24),
            (120, 20),
            (120, 40),
            (100, 30),
        ] {
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
    #[ignore = "visual inspection only"]
    fn dump_120x20() {
        for r in render_at(120, 20) {
            eprintln!("{r}");
        }
    }

    #[test]
    fn compact_cluster_renders_at_120x20() {
        let joined = render_at(120, 20).join("\n");
        assert!(joined.contains("USAGE / OVERVIEW"), "header:\n{joined}");
        assert!(joined.contains("READY"), "status:\n{joined}");
        assert!(joined.contains("CONTEXT"), "context:\n{joined}");
        assert!(joined.contains("CLAUDE"), "claude:\n{joined}");
        assert!(joined.contains("CODEX"), "codex:\n{joined}");
        assert!(joined.contains("5H LIMIT"), "primary tape:\n{joined}");
        assert!(joined.contains("7D LIMIT"), "secondary tape:\n{joined}");
    }

    #[test]
    fn full_cluster_renders_at_120x40() {
        let joined = render_at(120, 40).join("\n");
        assert!(joined.contains("USAGE / OVERVIEW"), "header:\n{joined}");
        assert!(joined.contains("READY"), "status:\n{joined}");
        assert!(joined.contains("CONTEXT"), "context:\n{joined}");
        assert!(joined.contains("CLAUDE"), "claude:\n{joined}");
        assert!(joined.contains("CODEX"), "codex:\n{joined}");
    }
    #[test]
    fn missing_and_critical_data_remain_distinguishable() {
        for (width, height) in [(60, 20), (120, 20), (120, 40)] {
            let mut app = App::with_test_state(
                AppConfig::default(),
                CodexImportCache::default(),
                ClaudeImportDiagnostics::default(),
            );
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, &app)).unwrap();
            let contents = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(
                contents.contains("--") || contents.contains("┃-┃-┃"),
                "missing data must remain unknown"
            );
            assert!(!contents.contains("NOMINAL"));
            app.claude_cache.limits = Some(limits(100.0, 95.0));
            app.claude_cache.fetch_error = None;
            terminal.draw(|f| draw(f, &app)).unwrap();
            assert!(
                terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .any(|c| c.fg == CARBON.critical),
                "exhausted quota needs a warning at {width}x{height}"
            );
            if width == 60 {
                let contents = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect::<String>();
                assert!(
                    contents.contains("┃1┃0┃0┃%┃"),
                    "compact drum must show 100%, not 99%"
                );
            }
        }
    }

    #[test]
    fn help_and_controls_fit_normal_terminal_sizes() {
        for (width, height) in [(60, 20), (80, 24), (120, 20), (120, 40)] {
            let mut app = sample_app();
            app.show_help = true;
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, &app)).unwrap();
            let contents = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            for label in [
                "Refresh data",
                "Next color theme",
                "Close this panel",
                "Quit PromptPetrol",
            ] {
                assert!(
                    contents.contains(label),
                    "missing {label} at {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn capacity_composition_matches_the_context_reading() {
        let app = sample_app();
        let context = collect_context(&app.codex_cache).unwrap();
        assert_eq!(context.input_tokens, 57799);
        assert_eq!(context.output_tokens, 7217);
        assert_eq!(
            context.input_tokens + context.output_tokens,
            context.used_tokens
        );
        let contents = render_at(120, 40).join("\n");
        for label in ["Input 57.8K", "Output 7.2K", "Available 193.4K"] {
            assert!(contents.contains(label), "missing capacity legend: {label}");
        }
    }

    /// Export actual TestBackend cells, including their colors, for visual review.
    /// Synthetic sample data only; no credentials, API calls, or session reads.
    #[test]
    #[ignore = "writes SVG previews to PROMPTPETROL_PREVIEW_DIR or a temporary directory"]
    fn export_dashboard_previews() {
        use std::fmt::Write;
        let dir = std::env::var_os("PROMPTPETROL_PREVIEW_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("promptpetrol-previews"));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, width, height, theme) in [
            ("carbon", 120, 40, Theme::Carbon),
            ("medium", 120, 20, Theme::Carbon),
            ("compact", 60, 20, Theme::Carbon),
            ("narrow", 40, 12, Theme::Carbon),
            ("paper", 120, 40, Theme::Paper),
            ("murphy", 120, 40, Theme::Murphy),
            ("empty", 120, 40, Theme::Carbon),
            ("critical", 120, 40, Theme::Carbon),
            ("help", 120, 20, Theme::Carbon),
        ] {
            let mut app = sample_app_with_theme(theme);
            if name == "empty" {
                app = App::with_test_state(
                    AppConfig::default(),
                    CodexImportCache::default(),
                    ClaudeImportDiagnostics::default(),
                );
            } else if name == "critical" {
                app.claude_cache.limits = Some(limits(100.0, 75.0));
                app.codex_cache = CodexImportCache::with_test_context(250000, 0, 5000, 258400);
                app.codex_cache.latest_limits = Some(limits(91.0, 80.0));
            }
            app.show_help = name == "help";
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, &app)).unwrap();
            let mut svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}"><rect width="100%" height="100%" fill="{}"/><g font-family="Menlo, monospace" font-size="14">"#,
                width * 9 + 32,
                height * 19 + 32,
                svg_color(palette_for(theme).background)
            );
            for y in 0..height {
                for x in 0..width {
                    let cell = &terminal.backend().buffer()[(x, y)];
                    let px = 16 + x * 9;
                    let py = 16 + y * 19;
                    write!(
                        svg,
                        r#"<rect x="{px}" y="{py}" width="9" height="19" fill="{}"/>"#,
                        svg_color(cell.bg)
                    )
                    .unwrap();
                }
            }
            for y in 0..height {
                for x in 0..width {
                    let cell = &terminal.backend().buffer()[(x, y)];
                    if cell.symbol().trim().is_empty() {
                        continue;
                    }
                    let symbol = cell
                        .symbol()
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;");
                    write!(
                        svg,
                        r#"<text x="{}" y="{}" fill="{}" font-weight="{}">{symbol}</text>"#,
                        16 + x * 9,
                        30 + y * 19,
                        svg_color(cell.fg),
                        if cell.modifier.contains(Modifier::BOLD) {
                            "bold"
                        } else {
                            "normal"
                        }
                    )
                    .unwrap();
                }
            }
            svg.push_str("</g></svg>");
            std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap();
        }
        eprintln!("Dashboard previews: {}", dir.display());
    }

    fn svg_color(color: Color) -> String {
        match color {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Color::Reset => "#0c1017".into(),
            _ => "#e6edf5".into(),
        }
    }
}

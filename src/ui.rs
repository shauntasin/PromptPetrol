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
use crate::models::Theme;

const APP_NAME: &str = "PromptPetrol";

#[derive(Clone, Copy)]
struct Palette {
    background: Color,
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

// Neovim's built-in Murphy palette, resolved from the active highlight groups.
const MURPHY: Palette = Palette {
    background: Color::Rgb(0, 0, 0),
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

const PAPER: Palette = Palette {
    background: Color::Rgb(247, 243, 232),
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
        Theme::Murphy => MURPHY,
        Theme::Paper => PAPER,
        Theme::Arctic => ARCTIC,
        Theme::SolarizedLight => SOLARIZED_LIGHT,
    }
}

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
    used_tokens: u64,
    total_tokens: u64,
}

pub(crate) fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let palette = palette_for(app.active_theme());

    frame.render_widget(
        Block::default().style(Style::default().bg(palette.background)),
        area,
    );

    if area.width < 24 || area.height < 6 {
        frame.render_widget(
            Paragraph::new(format!("{APP_NAME} // DISPLAY AREA INSUFFICIENT"))
                .style(Style::default().fg(palette.normal).bg(palette.background)),
            area,
        );
        return;
    }

    let title = if app.config_error.is_some() {
        format!(
            " {APP_NAME} // CONFIG FAULT // {} ",
            app.active_theme().label()
        )
    } else {
        format!(
            " {APP_NAME} // RESOURCE MFD // {} ",
            app.active_theme().label()
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(palette.normal))
        .style(Style::default().bg(palette.background))
        .title(Line::styled(
            title,
            Style::default()
                .fg(palette.normal)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (claude_metrics, codex_metrics) = collect_metrics(&app.claude_cache, &app.codex_cache);
    let context = collect_context(&app.codex_cache);

    // Wide, shallow terminals keep the three-bay MFD silhouette with compressed
    // instruments. Narrow terminals use the dense odometer dashboard.
    if inner.width >= 90 && inner.height >= 24 {
        render_cluster(
            frame,
            inner,
            app,
            &claude_metrics,
            &codex_metrics,
            context.as_ref(),
            palette,
        );
    } else if inner.width >= 90 && inner.height >= 16 {
        render_medium_cluster(
            frame,
            inner,
            app,
            &claude_metrics,
            &codex_metrics,
            context.as_ref(),
            palette,
        );
    } else {
        render_dashboard(
            frame,
            inner,
            &claude_metrics,
            &codex_metrics,
            context.as_ref(),
            palette,
        );
    }

    if app.show_help {
        draw_help_overlay(frame, palette);
    }
}

/// Avionics-style multi-function display with provider bays flanking a central
/// resource scope.
fn render_cluster(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    claude: &[Metric],
    codex: &[Metric],
    context: Option<&Context>,
    palette: Palette,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(16),
            Constraint::Length(3),
        ])
        .split(area);

    render_mfd_header(frame, rows[0], app, palette);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Length(1),
            Constraint::Percentage(40),
            Constraint::Length(1),
            Constraint::Percentage(30),
        ])
        .split(rows[1]);

    render_provider_bay(frame, cols[0], "CLAUDE", "ANTHROPIC", claude, palette);
    render_rail(frame, cols[1], palette);
    render_context_scope(frame, cols[2], context, palette);
    render_rail(frame, cols[3], palette);
    render_provider_bay(frame, cols[4], "CODEX", "OPENAI", codex, palette);

    render_mfd_footer(frame, rows[2], app, palette);
}

/// Three-bay MFD for wide terminals that do not have enough vertical space for
/// the large digit drums. A 120x20 terminal has a 118x18 drawable interior.
fn render_medium_cluster(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    claude: &[Metric],
    codex: &[Metric],
    context: Option<&Context>,
    palette: Palette,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(12),
            Constraint::Length(2),
        ])
        .split(area);

    render_mfd_header(frame, rows[0], app, palette);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Length(1),
            Constraint::Percentage(40),
            Constraint::Length(1),
            Constraint::Percentage(30),
        ])
        .split(rows[1]);

    render_medium_provider_bay(frame, cols[0], "CLAUDE", "ANTHROPIC", claude, palette);
    render_rail(frame, cols[1], palette);
    render_context_scope(frame, cols[2], context, palette);
    render_rail(frame, cols[3], palette);
    render_medium_provider_bay(frame, cols[4], "CODEX", "OPENAI", codex, palette);

    render_mfd_footer(frame, rows[2], app, palette);
}

fn render_mfd_header(frame: &mut Frame<'_>, area: Rect, app: &App, palette: Palette) {
    let clock = chrono::Utc::now().format("%H:%M:%S Z").to_string();
    let mode = if app.config_error.is_some() {
        "CONFIG FAULT"
    } else if app.is_refreshing() {
        "DATA ACQ"
    } else {
        "LIMIT MONITOR"
    };
    let mode_color = if app.config_error.is_some() {
        palette.critical
    } else if app.is_refreshing() {
        palette.warning
    } else {
        palette.normal
    };

    let primary = fit_status_line(
        "PP-MFD 01 // TOKEN RESOURCE MANAGEMENT",
        &format!("{mode}  {clock}"),
        area.width as usize,
    );
    let secondary = fit_status_line(
        "DISPLAY: LIMIT WINDOWS + ACTIVE CONTEXT",
        "DLINK AUTO  /  SCALE 100",
        area.width as usize,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    primary.0,
                    Style::default()
                        .fg(palette.normal)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(primary.1),
                Span::styled(
                    primary.2,
                    Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(secondary.0, Style::default().fg(palette.warning)),
                Span::raw(secondary.1),
                Span::styled(secondary.2, Style::default().fg(palette.warning)),
            ]),
        ])
        .style(Style::default().bg(palette.background)),
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
    let online = metrics.iter().any(|metric| metric.percent.is_some());
    let link_color = if online { palette.normal } else { palette.dim };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(palette.separator))
        .style(Style::default().bg(palette.background))
        .title(Line::from(vec![
            Span::styled("◆ ", Style::default().fg(palette.decorative)),
            Span::styled(
                provider.to_string(),
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" // {source} "),
                Style::default().fg(palette.accent),
            ),
        ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(inner);

    let link = if online {
        "LINK / VALID"
    } else {
        "LINK / NO DATA"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" DATA ", Style::default().fg(palette.muted)),
                Span::styled(link, Style::default().fg(link_color)),
            ]),
            Line::styled(
                " ─────────────────────────────────",
                Style::default().fg(palette.separator),
            ),
        ]),
        rows[0],
    );

    if let Some(metric) = metrics.first() {
        render_metric_instrument(frame, rows[1], "5-HOUR WINDOW", metric, palette);
    }
    if let Some(metric) = metrics.get(1) {
        render_metric_instrument(frame, rows[2], "7-DAY WINDOW", metric, palette);
    }
}

fn render_medium_provider_bay(
    frame: &mut Frame<'_>,
    area: Rect,
    provider: &str,
    source: &str,
    metrics: &[Metric],
    palette: Palette,
) {
    let online = metrics.iter().any(|metric| metric.percent.is_some());
    let link_color = if online { palette.normal } else { palette.dim };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(palette.separator))
        .style(Style::default().bg(palette.background))
        .title(Line::from(vec![
            Span::styled("◆ ", Style::default().fg(palette.decorative)),
            Span::styled(
                provider.to_string(),
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" // {source} "),
                Style::default().fg(palette.accent),
            ),
        ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(inner);

    let link = if online { "VALID" } else { "NO DATA" };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" DATA LINK / ", Style::default().fg(palette.muted)),
            Span::styled(link, Style::default().fg(link_color)),
        ])),
        rows[0],
    );

    if let Some(metric) = metrics.first() {
        render_medium_metric(frame, rows[1], "5H LIMIT", metric, palette);
    }
    if let Some(metric) = metrics.get(1) {
        render_medium_metric(frame, rows[2], "7D LIMIT", metric, palette);
    }
}

fn render_medium_metric(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    metric: &Metric,
    palette: Palette,
) {
    let color = metric
        .percent
        .map(|percent| percent_color(percent, palette))
        .unwrap_or(palette.dim);
    let state = metric_state(metric.percent);
    let block = Block::default()
        .borders(Borders::TOP)
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(palette.separator))
        .style(Style::default().bg(palette.background))
        .title(Line::styled(
            format!(" {label} "),
            Style::default().fg(palette.label),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let percent = metric
        .percent
        .map(|value| format!("{:03.0}%", value.clamp(0.0, 999.0)))
        .unwrap_or_else(|| "---%".into());
    let readout = fit_status_line(&percent, state, inner.width as usize);
    let bar_width = inner.width as usize;
    let reset = metric
        .note
        .strip_prefix("resets ")
        .unwrap_or(&metric.note)
        .to_ascii_uppercase();
    let reset_line = fit_status_line("RESET", &reset, inner.width as usize);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    readout.0,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(readout.1),
                Span::styled(
                    readout.2,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(bar_span(metric.percent.unwrap_or(0.0), bar_width, color)),
            Line::styled(
                scale_labels(bar_width),
                Style::default().fg(palette.warning),
            ),
            Line::from(vec![
                Span::styled(reset_line.0, Style::default().fg(palette.muted)),
                Span::raw(reset_line.1),
                Span::styled(reset_line.2, Style::default().fg(palette.text)),
            ]),
        ]),
        inner,
    );
}

fn render_metric_instrument(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    metric: &Metric,
    palette: Palette,
) {
    if area.height < 6 || area.width < 22 {
        render_compact_metric(frame, area, metric, palette);
        return;
    }

    let color = metric
        .percent
        .map(|percent| percent_color(percent, palette))
        .unwrap_or(palette.dim);
    let state = metric_state(metric.percent);
    let block = Block::default()
        .borders(Borders::TOP)
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(palette.separator))
        .style(Style::default().bg(palette.background))
        .title(Line::styled(
            format!(" {label} "),
            Style::default().fg(palette.label),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let readout_width = if inner.width >= 32 { 16 } else { 12 };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(readout_width), Constraint::Min(8)])
        .split(inner);

    let large = large_percent_lines(metric.percent);
    let vertical_pad = u16::from(inner.height >= 5);
    let mut readout_lines = vec![Line::raw(""); vertical_pad as usize];
    readout_lines.extend(large.into_iter().map(|line| {
        Line::styled(
            line,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )
        .centered()
    }));
    readout_lines.push(Line::styled("PERCENT", Style::default().fg(palette.muted)).centered());
    frame.render_widget(Paragraph::new(readout_lines), cols[0]);

    let bar_width = cols[1].width.saturating_sub(2) as usize;
    let reset = metric
        .note
        .strip_prefix("resets ")
        .unwrap_or(&metric.note)
        .to_ascii_uppercase();
    let mut data_lines = vec![
        Line::styled(
            state,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Line::styled("UTILIZATION", Style::default().fg(palette.muted)),
        Line::from(bar_span(
            metric.percent.unwrap_or(0.0),
            bar_width.max(3),
            color,
        )),
        Line::styled(
            scale_labels(bar_width.max(3)),
            Style::default().fg(palette.warning),
        ),
    ];
    if inner.height >= 6 {
        data_lines.push(Line::styled(
            "RESET / STATUS",
            Style::default().fg(palette.muted),
        ));
        data_lines.push(Line::styled(
            truncate(&reset, cols[1].width as usize),
            Style::default().fg(if is_blank(&reset) {
                palette.dim
            } else {
                palette.text
            }),
        ));
    }
    frame.render_widget(Paragraph::new(data_lines), cols[1]);
}

fn render_context_scope(
    frame: &mut Frame<'_>,
    area: Rect,
    context: Option<&Context>,
    palette: Palette,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(palette.normal))
        .style(Style::default().bg(palette.background))
        .title(
            Line::styled(
                " ◇ CONTEXT // ACTIVE SESSION ",
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            )
            .centered(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let pct = context.map(|value| value.percent).unwrap_or(0.0);
    let color = context
        .map(|_| percent_color(pct, palette))
        .unwrap_or(palette.dim);
    let canvas = Canvas::default()
        .x_bounds([-1.7, 1.7])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| draw_context_scope(ctx, pct, color, palette));
    frame.render_widget(canvas, inner);

    let center_width = inner.width.min(24);
    let center_height = inner.height.min(8);
    let center = Rect {
        x: inner.x + inner.width.saturating_sub(center_width) / 2,
        y: inner.y + inner.height.saturating_sub(center_height) / 2,
        width: center_width,
        height: center_height,
    };
    frame.render_widget(Clear, center);
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.background)),
        center,
    );

    let lines = match context {
        Some(value) => {
            let reserve = value.total_tokens.saturating_sub(value.used_tokens) / 1000;
            vec![
                Line::styled("SESSION LOAD", Style::default().fg(palette.accent)).centered(),
                Line::styled(
                    format!("{:03.0}%", value.percent.min(999.0)),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )
                .centered(),
                Line::styled("────────────────", Style::default().fg(palette.separator)).centered(),
                Line::styled(value.detail.clone(), Style::default().fg(palette.text)).centered(),
                Line::styled(
                    format!("RESERVE {reserve}K"),
                    Style::default().fg(palette.normal),
                )
                .centered(),
                Line::styled(
                    metric_state(Some(value.percent)),
                    Style::default().fg(color),
                )
                .centered(),
            ]
        }
        None => vec![
            Line::styled("SESSION LOAD", Style::default().fg(palette.accent)).centered(),
            Line::styled("---%", Style::default().fg(palette.dim)).centered(),
            Line::styled("NO ACTIVE TELEMETRY", Style::default().fg(palette.dim)).centered(),
        ],
    };
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(palette.background)),
        center,
    );
}

fn draw_context_scope(ctx: &mut CanvasContext, pct: f64, color: Color, palette: Palette) {
    let ratio = (pct / 100.0).clamp(0.0, 1.0);
    let steps = 144;
    for i in 0..steps {
        let a = (-90.0 + i as f64 / steps as f64 * 360.0).to_radians();
        let b = (-90.0 + (i + 1) as f64 / steps as f64 * 360.0).to_radians();
        let segment_color = if i as f64 / steps as f64 <= ratio {
            color
        } else {
            palette.separator
        };
        ctx.draw(&CanvasLine {
            x1: a.cos() * 1.32,
            y1: a.sin() * 0.82,
            x2: b.cos() * 1.32,
            y2: b.sin() * 0.82,
            color: segment_color,
        });
    }

    for i in 0..36 {
        let angle = (-90.0 + i as f64 * 10.0).to_radians();
        let major = i % 3 == 0;
        let inner_x = if major { 1.10 } else { 1.18 };
        let inner_y = if major { 0.68 } else { 0.73 };
        ctx.draw(&CanvasLine {
            x1: angle.cos() * inner_x,
            y1: angle.sin() * inner_y,
            x2: angle.cos() * 1.25,
            y2: angle.sin() * 0.78,
            color: if major { palette.label } else { palette.dim },
        });
    }

    ctx.draw(&CanvasLine {
        x1: -0.42,
        y1: 0.0,
        x2: 0.42,
        y2: 0.0,
        color: palette.separator,
    });
    ctx.draw(&CanvasLine {
        x1: 0.0,
        y1: -0.25,
        x2: 0.0,
        y2: 0.25,
        color: palette.separator,
    });
}

fn render_rail(frame: &mut Frame<'_>, area: Rect, palette: Palette) {
    let rail = (0..area.height)
        .map(|index| {
            if index % 4 == 0 {
                Line::styled("◆", Style::default().fg(palette.decorative))
            } else {
                Line::styled("│", Style::default().fg(palette.separator))
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(rail), area);
}

fn render_mfd_footer(frame: &mut Frame<'_>, area: Rect, app: &App, palette: Palette) {
    let diagnostics = &app.codex_cache.diagnostics;
    let errors = diagnostics.parse_error_files + diagnostics.unreadable_files;
    let system_state = if app.config_error.is_some() || app.refresh_error.is_some() {
        "FAULT"
    } else if app.is_refreshing() {
        "ACQUIRING"
    } else {
        "NOMINAL"
    };
    let state_color = if system_state == "FAULT" {
        palette.critical
    } else if system_state == "ACQUIRING" {
        palette.warning
    } else {
        palette.normal
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(palette.separator))
        .style(Style::default().bg(palette.background));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let status = format!(
        "SYS {system_state}  |  CX FILES {:03}  REF {:03}  ERR {:02}  SCAN {:03}S",
        diagnostics.active_files,
        diagnostics.refreshed_files,
        errors,
        diagnostics.discovery_interval.as_secs()
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("▰ ", Style::default().fg(state_color)),
                Span::styled(
                    truncate(&status, inner.width as usize),
                    Style::default().fg(state_color),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "R",
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" REFRESH   ", Style::default().fg(palette.muted)),
                Span::styled(
                    "?",
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" CHECKLIST   ", Style::default().fg(palette.muted)),
                Span::styled(
                    "T",
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" THEME/{}   ", app.active_theme().label()),
                    Style::default().fg(palette.muted),
                ),
                Span::styled(
                    "Q",
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" EGRESS", Style::default().fg(palette.muted)),
            ]),
        ]),
        inner,
    );
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

fn scale_labels(width: usize) -> String {
    if width < 10 {
        return format!("0{:>width$}", "100", width = width.saturating_sub(1));
    }
    let middle = width.saturating_sub(7);
    format!(
        "0{}50{}100",
        " ".repeat(middle / 2),
        " ".repeat(middle - middle / 2)
    )
}

fn fit_status_line(left: &str, right: &str, width: usize) -> (String, String, String) {
    let left = truncate(left, width.saturating_sub(right.chars().count() + 1));
    let gap = width.saturating_sub(left.chars().count() + right.chars().count());
    (left, " ".repeat(gap), truncate(right, width))
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
        used_tokens: tokens,
        total_tokens: window,
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
        render_metric_column(frame, cols[0], claude, palette);
        render_metric_column(frame, cols[1], codex, palette);
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
        truncate(&metric.title, rw as usize),
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

fn percent_color(pct: f64, palette: Palette) -> Color {
    if pct >= 90.0 {
        palette.critical
    } else if pct >= 70.0 {
        palette.warning
    } else {
        palette.normal
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

fn draw_help_overlay(frame: &mut Frame<'_>, palette: Palette) {
    let area = frame.area();
    let pct_x = if area.width < 40 { 90 } else { 50 };
    let pct_y = if area.height < 16 { 80 } else { 40 };
    let overlay = centered_rect(pct_x, pct_y, area);

    let help_lines = vec![
        Line::styled(
            "CONTROL INPUTS",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled("R", Style::default().fg(palette.label)),
            Span::styled("  REFRESH DATA LINK", Style::default().fg(palette.text)),
        ]),
        Line::from(vec![
            Span::styled("?", Style::default().fg(palette.label)),
            Span::styled("  CLOSE CHECKLIST", Style::default().fg(palette.text)),
        ]),
        Line::from(vec![
            Span::styled("T", Style::default().fg(palette.label)),
            Span::styled("  CYCLE COLOR THEME", Style::default().fg(palette.text)),
        ]),
        Line::from(vec![
            Span::styled("Q", Style::default().fg(palette.warning)),
            Span::styled("  EGRESS DISPLAY", Style::default().fg(palette.text)),
        ]),
    ];

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(help_lines)
            .style(Style::default().bg(palette.background))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_set(border::DOUBLE)
                    .border_style(Style::default().fg(palette.decorative))
                    .title(" SYSTEM CHECKLIST "),
            ),
        overlay,
    );
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
            Theme::Murphy,
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
                assert_eq!(title_cell.fg, palette.normal, "{theme:?} primary");
            }
        }
    }

    #[test]
    fn rows_match_terminal_width_at_all_sizes() {
        for (w, h) in [
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
    fn medium_mfd_renders_at_120x20() {
        let joined = render_at(120, 20).join("\n");
        assert!(joined.contains("PP-MFD 01"), "header:\n{joined}");
        assert!(joined.contains("SYS NOMINAL"), "status:\n{joined}");
        assert!(joined.contains("CONTEXT"), "context:\n{joined}");
        assert!(joined.contains("CLAUDE"), "claude:\n{joined}");
        assert!(joined.contains("CODEX"), "codex:\n{joined}");
        assert!(joined.contains("5H LIMIT"), "primary tape:\n{joined}");
        assert!(joined.contains("7D LIMIT"), "secondary tape:\n{joined}");
    }

    #[test]
    fn avionics_mfd_renders_at_120x40() {
        let joined = render_at(120, 40).join("\n");
        assert!(joined.contains("PP-MFD 01"), "header:\n{joined}");
        assert!(joined.contains("SYS NOMINAL"), "status:\n{joined}");
        assert!(joined.contains("CONTEXT"), "context:\n{joined}");
        assert!(joined.contains("CLAUDE"), "claude:\n{joined}");
        assert!(joined.contains("CODEX"), "codex:\n{joined}");
    }
}

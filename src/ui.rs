use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::codex_import::{
    CodexRateLimit, CodexRateLimits, codex_import_diagnostics, latest_codex_limits,
};
use crate::models::{provider_stats, provider_summaries};

const APP_NAME: &str = "PromptPetrol";

pub(crate) fn draw(frame: &mut Frame<'_>, app: &App) {
    let providers = provider_summaries(&app.data);
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Min(8),
        ])
        .split(area);

    let selected_provider = app.selected_provider.as_deref().unwrap_or("");
    let selected_stats = provider_stats(&app.data, selected_provider);
    let max_cost = providers
        .iter()
        .map(|p| p.total_cost_usd)
        .fold(0.0_f64, f64::max);
    let max_tokens = providers
        .iter()
        .map(|p| p.total_tokens)
        .fold(0_u64, u64::max);

    let budget_ratio = match (selected_stats.as_ref(), app.data.budget_usd) {
        (Some(provider), Some(budget)) if budget > 0.0 => {
            (provider.total_cost_usd / budget).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    let token_ratio = selected_stats
        .as_ref()
        .map(|provider| {
            if max_tokens == 0 {
                0.0
            } else {
                (provider.total_tokens as f64 / max_tokens as f64).clamp(0.0, 1.0)
            }
        })
        .unwrap_or(0.0);
    let spend_ratio = selected_stats
        .as_ref()
        .map(|provider| {
            if max_cost <= f64::EPSILON {
                0.0
            } else {
                (provider.total_cost_usd / max_cost).clamp(0.0, 1.0)
            }
        })
        .unwrap_or(0.0);
    let activity_ratio = selected_stats
        .as_ref()
        .map(|provider| {
            let total_requests = app.data.entries.len();
            if total_requests == 0 {
                0.0
            } else {
                (provider.requests as f64 / total_requests as f64).clamp(0.0, 1.0)
            }
        })
        .unwrap_or(0.0);
    let fuel_ratio = (1.0 - budget_ratio).clamp(0.0, 1.0);
    let is_codex = selected_provider == "codex";
    let codex_limits = if is_codex {
        latest_codex_limits(&app.codex_cache)
    } else {
        None
    };
    let codex_import_age_secs = if is_codex {
        codex_import_diagnostics(&app.codex_cache)
            .last_import_at
            .and_then(|timestamp| SystemTime::now().duration_since(timestamp).ok())
            .map(|duration| duration.as_secs())
    } else {
        None
    };

    let basic_line = if let Some(provider) = selected_stats.as_ref() {
        if is_codex {
            format!(
                "{APP_NAME} | codex/{} | {} tok | {} req",
                app.config.codex_import.model, provider.total_tokens, provider.requests
            )
        } else {
            format!(
                "{APP_NAME} | {} | ${:.3} | {} tok | {} req",
                provider.provider,
                provider.total_cost_usd,
                provider.total_tokens,
                provider.requests
            )
        }
    } else {
        format!("{APP_NAME} | No provider data")
    };
    let info_line = if app.status.is_empty() {
        basic_line
    } else {
        format!("{basic_line} | {}", app.status)
    };
    let alert_lines = if is_codex {
        build_codex_alert_lines(codex_limits.as_ref(), codex_import_age_secs)
    } else {
        build_alert_lines(fuel_ratio, token_ratio, spend_ratio, activity_ratio)
    };
    frame.render_widget(
        Paragraph::new(info_line).block(Block::default().borders(Borders::ALL).title("Info")),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(alert_lines).block(Block::default().borders(Borders::ALL).title("Alerts")),
        chunks[1],
    );

    if is_codex {
        let codex_gauges = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[2]);
        let five_hour_ratio = codex_limits
            .as_ref()
            .and_then(|limits| limits.primary.as_ref())
            .map(|limit| (limit.used_percent / 100.0).clamp(0.0, 1.0))
            .unwrap_or(0.0);
        let weekly_ratio = codex_limits
            .as_ref()
            .and_then(|limits| limits.secondary.as_ref())
            .map(|limit| (limit.used_percent / 100.0).clamp(0.0, 1.0))
            .unwrap_or(0.0);
        render_analog_gauge(frame, codex_gauges[0], "5h Limit", five_hour_ratio, "used");
        render_analog_gauge(frame, codex_gauges[1], "Weekly Limit", weekly_ratio, "used");
    } else {
        let gauge_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[2]);
        let top_gauges = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(gauge_rows[0]);
        let bottom_gauges = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(gauge_rows[1]);

        render_analog_gauge(frame, top_gauges[0], "Fuel Tank", fuel_ratio, "left");
        render_analog_gauge(frame, top_gauges[1], "RPM", token_ratio, "load");
        render_analog_gauge(frame, bottom_gauges[0], "Throttle", spend_ratio, "burn");
        render_analog_gauge(frame, bottom_gauges[1], "Traffic", activity_ratio, "flow");
    }

    if app.show_help {
        draw_help_overlay(frame);
    }
}

fn render_analog_gauge(frame: &mut Frame<'_>, area: Rect, title: &str, ratio: f64, unit: &str) {
    let ratio = ratio.clamp(0.0, 1.0);
    let gauge_color = if ratio >= 0.9 {
        Color::Red
    } else if ratio >= 0.7 {
        Color::Yellow
    } else {
        Color::Cyan
    };
    let dial_block = Block::default().borders(Borders::ALL).title(title);

    frame.render_widget(
        Canvas::default()
            .block(dial_block)
            .x_bounds([-1.2, 1.2])
            .y_bounds([-1.2, 1.2])
            .paint(|ctx| {
                ctx.draw(&Circle {
                    x: 0.0,
                    y: 0.0,
                    radius: 1.0,
                    color: Color::DarkGray,
                });

                for step in 0..=10 {
                    let tick_ratio = step as f64 / 10.0;
                    let tick_angle = 225.0 - (270.0 * tick_ratio);
                    let tick_rad = tick_angle.to_radians();
                    let (outer_x, outer_y) = (tick_rad.cos() * 0.96, tick_rad.sin() * 0.96);
                    let (inner_x, inner_y) = (tick_rad.cos() * 0.82, tick_rad.sin() * 0.82);
                    ctx.draw(&CanvasLine {
                        x1: inner_x,
                        y1: inner_y,
                        x2: outer_x,
                        y2: outer_y,
                        color: Color::Gray,
                    });
                }

                let angle_deg = 225.0 - (270.0 * ratio);
                let angle = angle_deg.to_radians();
                let (needle_x, needle_y) = (angle.cos() * 0.76, angle.sin() * 0.76);
                ctx.draw(&CanvasLine {
                    x1: 0.0,
                    y1: 0.0,
                    x2: needle_x,
                    y2: needle_y,
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

    let value_text = format!("{:>5.1}% {unit}", ratio * 100.0);
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

fn build_alert_lines(
    fuel_ratio: f64,
    token_ratio: f64,
    spend_ratio: f64,
    activity_ratio: f64,
) -> Vec<Line<'static>> {
    vec![
        alert_line("LOW FUEL", fuel_ratio <= 0.20, fuel_ratio, true),
        alert_line("HIGH RPM", token_ratio >= 0.85, token_ratio, false),
        alert_line("OVERBURN", spend_ratio >= 0.85, spend_ratio, false),
        alert_line("TRAFFIC JAM", activity_ratio >= 0.90, activity_ratio, false),
    ]
}

fn alert_line(label: &str, alert: bool, ratio: f64, low_is_bad: bool) -> Line<'static> {
    let ratio_pct = ratio * 100.0;
    if alert {
        return Line::from(vec![
            Span::styled(
                format!(" {label:<11} "),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  ALERT  ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {:>5.1}%", ratio_pct),
                Style::default().fg(Color::Red),
            ),
        ]);
    }

    let healthy = if low_is_bad {
        ratio >= 0.35
    } else {
        ratio <= 0.70
    };
    let state = if healthy { "NOMINAL" } else { "WATCH  " };
    let state_bg = if healthy { Color::Green } else { Color::Yellow };

    Line::from(vec![
        Span::styled(format!(" {label:<11} "), Style::default().fg(Color::Gray)),
        Span::styled(
            format!(" {state} "),
            Style::default()
                .fg(Color::Black)
                .bg(state_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {:>5.1}%", ratio_pct),
            Style::default().fg(Color::Cyan),
        ),
    ])
}

fn build_codex_alert_lines(
    limits: Option<&CodexRateLimits>,
    import_age_secs: Option<u64>,
) -> Vec<Line<'static>> {
    let Some(limits) = limits else {
        return vec![
            Line::from(Span::styled(
                " Codex rate limits unavailable ",
                Style::default().fg(Color::Yellow),
            )),
            codex_freshness_line(import_age_secs),
        ];
    };

    vec![
        codex_alert_line("5H LIMIT", limits.primary.as_ref()),
        codex_alert_line("WEEKLY", limits.secondary.as_ref()),
        codex_freshness_line(import_age_secs),
    ]
}

fn codex_freshness_line(import_age_secs: Option<u64>) -> Line<'static> {
    let Some(age_secs) = import_age_secs else {
        return Line::from(vec![
            Span::styled(" FRESHNESS ", Style::default().fg(Color::Gray)),
            Span::styled(
                " UNKNOWN ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
    };

    let (state, color) = if age_secs <= 30 {
        ("LIVE", Color::Green)
    } else if age_secs <= 120 {
        ("STALE", Color::Yellow)
    } else {
        ("OLD", Color::Red)
    };

    Line::from(vec![
        Span::styled(" FRESHNESS ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!(" {:<7} ", state),
            Style::default()
                .fg(Color::Black)
                .bg(color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" updated {age_secs}s ago"),
            Style::default().fg(Color::Cyan),
        ),
    ])
}

fn codex_alert_line(label: &str, limit: Option<&CodexRateLimit>) -> Line<'static> {
    let Some(limit) = limit else {
        return Line::from(vec![
            Span::styled(format!(" {label:<8} "), Style::default().fg(Color::Gray)),
            Span::styled(
                " UNAVAILABLE ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
    };

    let ratio = (limit.used_percent / 100.0).clamp(0.0, 1.0);
    let state = if ratio >= 0.9 {
        ("ALERT", Color::Red)
    } else if ratio >= 0.75 {
        ("WATCH", Color::Yellow)
    } else {
        ("NOMINAL", Color::Green)
    };

    Line::from(vec![
        Span::styled(format!(" {label:<8} "), Style::default().fg(Color::Gray)),
        Span::styled(
            format!(" {:<7} ", state.0),
            Style::default()
                .fg(Color::Black)
                .bg(state.1)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {:>5.1}% ", limit.used_percent),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            format!(
                "{}m reset {}",
                limit.window_minutes,
                format_reset_timing(limit.resets_at)
            ),
            Style::default().fg(Color::Yellow),
        ),
    ])
}

fn format_reset_timing(resets_at: Option<u64>) -> String {
    let Some(target_epoch) = resets_at else {
        return "unknown".to_string();
    };
    let now_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if target_epoch <= now_epoch {
        return "now".to_string();
    }

    let remaining = target_epoch - now_epoch;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    format!("in {hours}h {minutes}m")
}

fn draw_help_overlay(frame: &mut Frame<'_>) {
    let area = centered_rect(60, 40, frame.area());
    let help_lines = vec![
        Line::from("Controls"),
        Line::from("q : quit"),
        Line::from("r : reload usage/config"),
        Line::from("Left/h/k : previous provider"),
        Line::from("Right/l/j : next provider"),
        Line::from("? : toggle help"),
    ];

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(help_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Keyboard Help"),
        ),
        area,
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

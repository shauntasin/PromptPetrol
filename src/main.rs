use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::time::{Duration, Instant};
use std::time::UNIX_EPOCH;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const APP_NAME: &str = "PromptPetrol";
const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsageEntry {
    timestamp: String,
    provider: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsageData {
    budget_usd: Option<f64>,
    entries: Vec<UsageEntry>,
}

impl Default for UsageData {
    fn default() -> Self {
        Self {
            budget_usd: Some(50.0),
            entries: vec![
                UsageEntry {
                    timestamp: "2026-02-09T08:45:00Z".to_string(),
                    provider: "openai".to_string(),
                    model: "gpt-4.1-mini".to_string(),
                    input_tokens: 7_600,
                    output_tokens: 2_400,
                    cost_usd: 0.084,
                },
                UsageEntry {
                    timestamp: "2026-02-09T13:30:00Z".to_string(),
                    provider: "anthropic".to_string(),
                    model: "claude-3.7-sonnet".to_string(),
                    input_tokens: 10_400,
                    output_tokens: 5_800,
                    cost_usd: 0.361,
                },
                UsageEntry {
                    timestamp: "2026-02-10T03:15:00Z".to_string(),
                    provider: "gemini".to_string(),
                    model: "gemini-2.0-flash".to_string(),
                    input_tokens: 5_300,
                    output_tokens: 1_200,
                    cost_usd: 0.056,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelPricing {
    input_per_million_usd: f64,
    output_per_million_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    #[serde(default)]
    api_keys: HashMap<String, String>,
    #[serde(default)]
    pricing: HashMap<String, ModelPricing>,
    #[serde(default)]
    codex_import: CodexImportConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut api_keys = HashMap::new();
        api_keys.insert("openai".to_string(), "<set-openai-key>".to_string());
        api_keys.insert("anthropic".to_string(), "<set-anthropic-key>".to_string());
        api_keys.insert("gemini".to_string(), "<set-gemini-key>".to_string());
        api_keys.insert("codex".to_string(), "<set-codex-key>".to_string());
        api_keys.insert("opus".to_string(), "<set-opus-key>".to_string());

        let mut pricing = HashMap::new();
        pricing.insert(
            "openai/gpt-4.1-mini".to_string(),
            ModelPricing {
                input_per_million_usd: 0.40,
                output_per_million_usd: 1.60,
            },
        );
        pricing.insert(
            "anthropic/claude-3.7-sonnet".to_string(),
            ModelPricing {
                input_per_million_usd: 3.00,
                output_per_million_usd: 15.00,
            },
        );
        pricing.insert(
            "gemini/gemini-2.0-flash".to_string(),
            ModelPricing {
                input_per_million_usd: 0.35,
                output_per_million_usd: 1.05,
            },
        );

        Self {
            api_keys,
            pricing,
            codex_import: CodexImportConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexImportConfig {
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    sessions_dir: Option<String>,
    #[serde(default = "default_codex_model")]
    model: String,
}

impl Default for CodexImportConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sessions_dir: None,
            model: default_codex_model(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_codex_model() -> String {
    "codex-cli".to_string()
}

#[derive(Debug, Clone, Deserialize)]
struct RawUsageData {
    budget_usd: Option<f64>,
    entries: Vec<RawUsageEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawUsageEntry {
    timestamp: String,
    provider: String,
    model: String,
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    request_tokens: Option<u64>,
    #[serde(default)]
    response_tokens: Option<u64>,
    #[serde(default)]
    prompt_token_count: Option<u64>,
    #[serde(default)]
    candidates_token_count: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
    #[serde(default)]
    total_token_count: Option<u64>,
    #[serde(default)]
    cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
struct ProviderSummary {
    provider: String,
    total_tokens: u64,
    total_cost_usd: f64,
}

#[derive(Debug, Clone)]
struct ProviderStats {
    provider: String,
    total_tokens: u64,
    total_cost_usd: f64,
    requests: usize,
}

fn provider_summaries(data: &UsageData) -> Vec<ProviderSummary> {
    let mut grouped: HashMap<String, (u64, f64)> = HashMap::new();
    for entry in &data.entries {
        let current = grouped.entry(entry.provider.clone()).or_insert((0, 0.0));
        current.0 += entry.input_tokens + entry.output_tokens;
        current.1 += entry.cost_usd;
    }

    let mut summaries = grouped
        .into_iter()
        .map(
            |(provider, (total_tokens, total_cost_usd))| ProviderSummary {
                provider,
                total_tokens,
                total_cost_usd,
            },
        )
        .collect::<Vec<_>>();
    summaries.sort_by(|a, b| {
        b.total_cost_usd
            .partial_cmp(&a.total_cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.total_tokens.cmp(&a.total_tokens))
            .then_with(|| a.provider.cmp(&b.provider))
    });
    summaries
}

struct App {
    data_file: PathBuf,
    config_file: PathBuf,
    config: AppConfig,
    data: UsageData,
    selected_provider: Option<String>,
    status: String,
    codex_cache: CodexImportCache,
}

impl App {
    fn new(data_file: PathBuf, config_file: PathBuf) -> Result<Self> {
        let config = load_or_bootstrap_config(&config_file)?;
        let mut data = load_or_bootstrap_data(&data_file, &config)?;
        let mut codex_cache = CodexImportCache::default();
        merge_codex_usage(&mut data, &config, &mut codex_cache);
        Ok(Self {
            data_file,
            config_file,
            config,
            data,
            selected_provider: None,
            status: "Ready".to_string(),
            codex_cache,
        }
        .with_selected_provider())
    }

    fn reload(&mut self) {
        match load_or_bootstrap_config(&self.config_file) {
            Ok(config) => {
                self.config = config;
            }
            Err(err) => {
                self.status = format!("Reload failed: {err}");
                return;
            }
        }

        match load_or_bootstrap_data(&self.data_file, &self.config) {
            Ok(mut data) => {
                merge_codex_usage(&mut data, &self.config, &mut self.codex_cache);
                self.data = data;
                self.sync_selected_provider();
                self.status = format!(
                    "Reloaded data: {} | config: {}",
                    self.data_file.display(),
                    self.config_file.display()
                );
            }
            Err(err) => {
                self.status = format!("Reload failed: {err}");
            }
        }
    }

    fn with_selected_provider(mut self) -> Self {
        self.sync_selected_provider();
        self
    }

    fn provider_names(&self) -> Vec<String> {
        provider_summaries(&self.data)
            .into_iter()
            .map(|summary| summary.provider)
            .collect()
    }

    fn sync_selected_provider(&mut self) {
        let providers = self.provider_names();
        if providers.is_empty() {
            self.selected_provider = None;
            return;
        }

        if let Some(selected) = self.selected_provider.as_ref() {
            if providers.iter().any(|name| name == selected) {
                return;
            }
        }
        self.selected_provider = providers.first().cloned();
    }

    fn select_next_provider(&mut self) {
        let providers = self.provider_names();
        if providers.is_empty() {
            self.selected_provider = None;
            return;
        }

        let current = self
            .selected_provider
            .as_ref()
            .and_then(|name| providers.iter().position(|p| p == name))
            .unwrap_or(0);
        let next = (current + 1) % providers.len();
        self.selected_provider = providers.get(next).cloned();
    }

    fn select_prev_provider(&mut self) {
        let providers = self.provider_names();
        if providers.is_empty() {
            self.selected_provider = None;
            return;
        }

        let current = self
            .selected_provider
            .as_ref()
            .and_then(|name| providers.iter().position(|p| p == name))
            .unwrap_or(0);
        let prev = if current == 0 {
            providers.len() - 1
        } else {
            current - 1
        };
        self.selected_provider = providers.get(prev).cloned();
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let data_file = default_data_file()?;
    let config_file = default_config_file()?;
    let mut app = App::new(data_file, config_file)?;
    let terminal = init_terminal()?;
    let result = run(terminal, &mut app);
    restore_terminal()?;
    result
}

fn run(mut terminal: DefaultTerminal, app: &mut App) -> Result<()> {
    let mut last_refresh = Instant::now();
    loop {
        terminal.draw(|frame| draw(frame, app))?;

        let elapsed = last_refresh.elapsed();
        let timeout = if elapsed >= AUTO_REFRESH_INTERVAL {
            Duration::from_millis(0)
        } else {
            AUTO_REFRESH_INTERVAL - elapsed
        };

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.code == KeyCode::Char('q') => break,
                Event::Key(key) if key.code == KeyCode::Char('r') => {
                    app.reload();
                    last_refresh = Instant::now();
                }
                Event::Key(key)
                    if matches!(
                        key.code,
                        KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('k')
                    ) =>
                {
                    app.select_prev_provider();
                    app.status = "Selected previous provider".to_string();
                }
                Event::Key(key)
                    if matches!(
                        key.code,
                        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('j')
                    ) =>
                {
                    app.select_next_provider();
                    app.status = "Selected next provider".to_string();
                }
                _ => {}
            }
            continue;
        }

        if last_refresh.elapsed() >= AUTO_REFRESH_INTERVAL {
            app.reload();
            last_refresh = Instant::now();
        }
    }
    Ok(())
}

fn draw(frame: &mut Frame<'_>, app: &App) {
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

    let basic_line = if let Some(provider) = selected_stats.as_ref() {
        if is_codex {
            format!(
                "{APP_NAME} | codex/{} | {} tok | {} req",
                app.config.codex_import.model, provider.total_tokens, provider.requests
            )
        } else {
            format!(
                "{APP_NAME} | {} | ${:.3} | {} tok | {} req",
                provider.provider, provider.total_cost_usd, provider.total_tokens, provider.requests
            )
        }
    } else {
        format!("{APP_NAME} | No provider data")
    };
    let alert_lines = if is_codex {
        build_codex_alert_lines(codex_limits.as_ref())
    } else {
        build_alert_lines(fuel_ratio, token_ratio, spend_ratio, activity_ratio)
    };
    frame.render_widget(
        Paragraph::new(basic_line).block(Block::default().borders(Borders::ALL).title("Info")),
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
        Span::styled(
            format!(" {label:<11} "),
            Style::default().fg(Color::Gray),
        ),
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

fn build_codex_alert_lines(limits: Option<&CodexRateLimits>) -> Vec<Line<'static>> {
    let Some(limits) = limits else {
        return vec![Line::from(Span::styled(
            " Codex rate limits unavailable ",
            Style::default().fg(Color::Yellow),
        ))];
    };

    vec![
        codex_alert_line("5H LIMIT", limits.primary.as_ref()),
        codex_alert_line("WEEKLY", limits.secondary.as_ref()),
    ]
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

fn provider_stats(data: &UsageData, provider: &str) -> Option<ProviderStats> {
    if provider.is_empty() {
        return None;
    }

    let mut total_input_tokens = 0_u64;
    let mut total_output_tokens = 0_u64;
    let mut total_cost_usd = 0.0_f64;
    let mut requests = 0_usize;

    for entry in &data.entries {
        if entry.provider != provider {
            continue;
        }
        total_input_tokens += entry.input_tokens;
        total_output_tokens += entry.output_tokens;
        total_cost_usd += entry.cost_usd;
        requests += 1;
    }

    if requests == 0 {
        return None;
    }

    Some(ProviderStats {
        provider: provider.to_string(),
        total_tokens: total_input_tokens + total_output_tokens,
        total_cost_usd,
        requests,
    })
}

fn init_terminal() -> Result<DefaultTerminal> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    Ok(ratatui::init())
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    ratatui::restore();
    Ok(())
}

fn default_config_base_dir() -> Result<PathBuf> {
    let base_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("promptpetrol");
    fs::create_dir_all(&base_dir)?;
    Ok(base_dir)
}

fn default_data_file() -> Result<PathBuf> {
    Ok(default_config_base_dir()?.join("usage.json"))
}

fn default_config_file() -> Result<PathBuf> {
    Ok(default_config_base_dir()?.join("config.json"))
}

fn load_or_bootstrap_config(path: &Path) -> Result<AppConfig> {
    if path.exists() {
        let contents = fs::read_to_string(path)?;
        let parsed = serde_json::from_str::<AppConfig>(&contents)?;
        Ok(parsed)
    } else {
        let seeded = AppConfig::default();
        let payload = serde_json::to_string_pretty(&seeded)?;
        fs::write(path, payload)?;
        Ok(seeded)
    }
}

fn load_or_bootstrap_data(path: &Path, config: &AppConfig) -> Result<UsageData> {
    if path.exists() {
        let contents = fs::read_to_string(path)?;
        if let Ok(parsed) = serde_json::from_str::<UsageData>(&contents) {
            return Ok(parsed);
        }

        let raw = serde_json::from_str::<RawUsageData>(&contents)?;
        Ok(normalize_raw_usage(raw, config))
    } else {
        let seeded = UsageData::default();
        let payload = serde_json::to_string_pretty(&seeded)?;
        fs::write(path, payload)?;
        Ok(seeded)
    }
}

fn normalize_raw_usage(raw: RawUsageData, config: &AppConfig) -> UsageData {
    let entries = raw
        .entries
        .into_iter()
        .map(|entry| normalize_entry(entry, config))
        .collect::<Vec<_>>();

    UsageData {
        budget_usd: raw.budget_usd,
        entries,
    }
}

#[derive(Debug, Clone)]
struct CachedCodexSession {
    modified: SystemTime,
    file_len: u64,
    timestamp: String,
    input_tokens: u64,
    output_tokens: u64,
    has_token_usage: bool,
    limits: Option<CodexRateLimits>,
}

#[derive(Debug, Clone)]
struct CodexRateLimit {
    used_percent: f64,
    window_minutes: u64,
    resets_at: Option<u64>,
}

#[derive(Debug, Clone)]
struct CodexRateLimits {
    timestamp: String,
    primary: Option<CodexRateLimit>,
    secondary: Option<CodexRateLimit>,
}

#[derive(Debug, Default)]
struct CodexImportCache {
    sessions: HashMap<PathBuf, CachedCodexSession>,
}

fn merge_codex_usage(data: &mut UsageData, config: &AppConfig, cache: &mut CodexImportCache) {
    if !config.codex_import.enabled {
        return;
    }

    let sessions_dir = codex_sessions_dir(config);
    let Some(session_files) = collect_codex_session_files(&sessions_dir) else {
        return;
    };

    let mut active = HashSet::new();
    for file in session_files {
        active.insert(file.clone());
        let (modified, file_len) = match fs::metadata(&file) {
            Ok(metadata) => match metadata.modified() {
                Ok(modified) => (modified, metadata.len()),
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        let needs_refresh = cache
            .sessions
            .get(&file)
            .map(|cached| cached.modified != modified || cached.file_len != file_len)
            .unwrap_or(true);

        if !needs_refresh {
            continue;
        }

        if let Some(parsed) = parse_codex_session_file(&file, modified, file_len) {
            cache.sessions.insert(file, parsed);
        } else {
            cache.sessions.remove(&file);
        }
    }

    cache.sessions.retain(|path, _| active.contains(path));

    let mut imported = cache
        .sessions
        .values()
        .filter(|session| session.has_token_usage)
        .map(|session| {
            let model = &config.codex_import.model;
            UsageEntry {
                timestamp: session.timestamp.clone(),
                provider: "codex".to_string(),
                model: model.clone(),
                input_tokens: session.input_tokens,
                output_tokens: session.output_tokens,
                cost_usd: estimate_cost_usd(
                    "codex",
                    model,
                    session.input_tokens,
                    session.output_tokens,
                    &config.pricing,
                ),
            }
        })
        .collect::<Vec<_>>();

    data.entries.append(&mut imported);
    data.entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
}

fn codex_sessions_dir(config: &AppConfig) -> PathBuf {
    if let Some(path) = config.codex_import.sessions_dir.as_ref() {
        return PathBuf::from(path);
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
        .join("sessions")
}

fn collect_codex_session_files(dir: &Path) -> Option<Vec<PathBuf>> {
    if !dir.exists() {
        return None;
    }

    let mut files = Vec::new();
    collect_jsonl_files_recursive(dir, &mut files).ok()?;
    Some(files)
}

fn collect_jsonl_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files_recursive(&path, files)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
    Ok(())
}

fn parse_codex_session_file(
    path: &Path,
    modified: SystemTime,
    file_len: u64,
) -> Option<CachedCodexSession> {
    let contents = fs::read_to_string(path).ok()?;
    let (timestamp, input_tokens, output_tokens, has_token_usage, limits) =
        parse_codex_session_contents(&contents)?;
    Some(CachedCodexSession {
        modified,
        file_len,
        timestamp,
        input_tokens,
        output_tokens,
        has_token_usage,
        limits,
    })
}

fn parse_codex_session_contents(
    contents: &str,
) -> Option<(String, u64, u64, bool, Option<CodexRateLimits>)> {
    let mut session_timestamp: Option<String> = None;
    let mut latest_event_timestamp: Option<String> = None;
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut has_token_usage = false;
    let mut latest_limits: Option<CodexRateLimits> = None;

    for line in contents.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if v.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(ts) = v.pointer("/payload/timestamp").and_then(Value::as_str) {
                session_timestamp = Some(ts.to_string());
            }
        }

        let is_token_count = v.get("type").and_then(Value::as_str) == Some("event_msg")
            && v.pointer("/payload/type").and_then(Value::as_str) == Some("token_count");

        if !is_token_count {
            continue;
        }

        let event_timestamp = v
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(ts) = event_timestamp.as_ref() {
            latest_event_timestamp = Some(ts.clone());
            let primary = parse_codex_rate_limit(v.pointer("/payload/rate_limits/primary"));
            let secondary = parse_codex_rate_limit(v.pointer("/payload/rate_limits/secondary"));
            if primary.is_some() || secondary.is_some() {
                latest_limits = Some(CodexRateLimits {
                    timestamp: ts.clone(),
                    primary,
                    secondary,
                });
            }
        }

        let maybe_input = v
            .pointer("/payload/info/total_token_usage/input_tokens")
            .and_then(Value::as_u64);
        let maybe_output = v
            .pointer("/payload/info/total_token_usage/output_tokens")
            .and_then(Value::as_u64);

        if let (Some(input), Some(output)) = (maybe_input, maybe_output) {
            input_tokens = input;
            output_tokens = output;
            has_token_usage = true;
        }
    }

    let timestamp = latest_event_timestamp.or(session_timestamp)?;
    if !has_token_usage && latest_limits.is_none() {
        return None;
    }
    Some((
        timestamp,
        input_tokens,
        output_tokens,
        has_token_usage,
        latest_limits,
    ))
}

fn parse_codex_rate_limit(node: Option<&Value>) -> Option<CodexRateLimit> {
    let node = node?;
    let used_percent = node
        .get("used_percent")
        .and_then(Value::as_f64)
        .or_else(|| {
            node.get("used_percent")
                .and_then(Value::as_u64)
                .map(|v| v as f64)
        })?;
    let window_minutes = node.get("window_minutes").and_then(Value::as_u64)?;
    let resets_at = node.get("resets_at").and_then(Value::as_u64);
    Some(CodexRateLimit {
        used_percent,
        window_minutes,
        resets_at,
    })
}

fn latest_codex_limits(cache: &CodexImportCache) -> Option<CodexRateLimits> {
    cache
        .sessions
        .values()
        .filter_map(|session| {
            session
                .limits
                .as_ref()
                .map(|limits| (session.modified, &limits.timestamp, limits))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)))
        .map(|(_, _, limits)| limits.clone())
}

fn normalize_entry(raw: RawUsageEntry, config: &AppConfig) -> UsageEntry {
    let provider = raw.provider.to_lowercase();
    let (input_tokens, output_tokens) = match provider.as_str() {
        "openai" => adapt_openai_tokens(&raw),
        "codex" => adapt_codex_tokens(&raw),
        "anthropic" => adapt_anthropic_tokens(&raw),
        "gemini" => adapt_gemini_tokens(&raw),
        "opus" => adapt_opus_tokens(&raw),
        _ => adapt_generic_tokens(&raw),
    };

    let cost_usd = raw.cost_usd.unwrap_or_else(|| {
        estimate_cost_usd(
            &provider,
            &raw.model,
            input_tokens,
            output_tokens,
            &config.pricing,
        )
    });

    UsageEntry {
        timestamp: raw.timestamp,
        provider,
        model: raw.model,
        input_tokens,
        output_tokens,
        cost_usd,
    }
}

fn adapt_openai_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    let input = raw
        .input_tokens
        .or(raw.prompt_tokens)
        .or(raw.request_tokens)
        .unwrap_or(0);
    let output = raw
        .output_tokens
        .or(raw.completion_tokens)
        .or(raw.response_tokens)
        .unwrap_or(0);
    split_with_total(input, output, raw.total_tokens)
}

fn adapt_codex_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    adapt_openai_tokens(raw)
}

fn adapt_anthropic_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    let input = raw
        .input_tokens
        .or(raw.prompt_tokens)
        .or(raw.request_tokens)
        .unwrap_or(0);
    let output = raw
        .output_tokens
        .or(raw.completion_tokens)
        .or(raw.response_tokens)
        .unwrap_or(0);
    split_with_total(input, output, raw.total_tokens)
}

fn adapt_gemini_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    let input = raw
        .input_tokens
        .or(raw.prompt_token_count)
        .or(raw.prompt_tokens)
        .unwrap_or(0);
    let output = raw
        .output_tokens
        .or(raw.candidates_token_count)
        .or(raw.completion_tokens)
        .unwrap_or(0);
    split_with_total(input, output, raw.total_tokens.or(raw.total_token_count))
}

fn adapt_opus_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    let input = raw
        .input_tokens
        .or(raw.prompt_tokens)
        .or(raw.prompt_token_count)
        .unwrap_or(0);
    let output = raw
        .output_tokens
        .or(raw.completion_tokens)
        .or(raw.candidates_token_count)
        .unwrap_or(0);
    split_with_total(input, output, raw.total_tokens.or(raw.total_token_count))
}

fn adapt_generic_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    let input = raw
        .input_tokens
        .or(raw.prompt_tokens)
        .or(raw.request_tokens)
        .or(raw.prompt_token_count)
        .unwrap_or(0);
    let output = raw
        .output_tokens
        .or(raw.completion_tokens)
        .or(raw.response_tokens)
        .or(raw.candidates_token_count)
        .unwrap_or(0);
    split_with_total(input, output, raw.total_tokens.or(raw.total_token_count))
}

fn split_with_total(input: u64, output: u64, total: Option<u64>) -> (u64, u64) {
    if input == 0 && output == 0 {
        if let Some(total) = total {
            let input_guess = total / 2;
            return (input_guess, total - input_guess);
        }
    }

    if let Some(total) = total {
        let known = input + output;
        if known == 0 {
            let input_guess = total / 2;
            return (input_guess, total - input_guess);
        }
        if known < total {
            return (input, output + (total - known));
        }
    }

    (input, output)
}

fn estimate_cost_usd(
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    pricing: &HashMap<String, ModelPricing>,
) -> f64 {
    if let Some(model_pricing) = lookup_pricing(pricing, provider, model) {
        return (input_tokens as f64 / 1_000_000.0) * model_pricing.input_per_million_usd
            + (output_tokens as f64 / 1_000_000.0) * model_pricing.output_per_million_usd;
    }

    0.0
}

fn lookup_pricing<'a>(
    pricing: &'a HashMap<String, ModelPricing>,
    provider: &str,
    model: &str,
) -> Option<&'a ModelPricing> {
    let exact = format!("{provider}/{model}");
    if let Some(found) = pricing.get(&exact) {
        return Some(found);
    }

    let wildcard = format!("{provider}/*");
    pricing.get(&wildcard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_openai_entry() {
        let raw = RawUsageData {
            budget_usd: Some(25.0),
            entries: vec![RawUsageEntry {
                timestamp: "2026-02-10T03:15:00Z".to_string(),
                provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
                input_tokens: None,
                output_tokens: None,
                prompt_tokens: Some(1200),
                completion_tokens: Some(300),
                request_tokens: None,
                response_tokens: None,
                prompt_token_count: None,
                candidates_token_count: None,
                total_tokens: None,
                total_token_count: None,
                cost_usd: None,
            }],
        };

        let normalized = normalize_raw_usage(raw, &AppConfig::default());
        assert_eq!(normalized.entries[0].input_tokens, 1200);
        assert_eq!(normalized.entries[0].output_tokens, 300);
        assert!(normalized.entries[0].cost_usd > 0.0);
    }

    #[test]
    fn normalizes_gemini_total_only() {
        let raw = RawUsageData {
            budget_usd: Some(25.0),
            entries: vec![RawUsageEntry {
                timestamp: "2026-02-10T03:15:00Z".to_string(),
                provider: "gemini".to_string(),
                model: "gemini-2.0-flash".to_string(),
                input_tokens: None,
                output_tokens: None,
                prompt_tokens: None,
                completion_tokens: None,
                request_tokens: None,
                response_tokens: None,
                prompt_token_count: None,
                candidates_token_count: None,
                total_tokens: None,
                total_token_count: Some(1000),
                cost_usd: None,
            }],
        };

        let normalized = normalize_raw_usage(raw, &AppConfig::default());
        assert_eq!(normalized.entries[0].input_tokens, 500);
        assert_eq!(normalized.entries[0].output_tokens, 500);
    }

    #[test]
    fn parses_codex_session_usage_from_token_count_events() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:42.927Z","type":"session_meta","payload":{"timestamp":"2026-02-16T09:45:42.927Z"}}
{"timestamp":"2026-02-16T09:45:53.237Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":8582,"output_tokens":210}}}}
{"timestamp":"2026-02-16T09:45:56.220Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":17438,"output_tokens":326}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex usage");
        assert_eq!(parsed.0, "2026-02-16T09:45:56.220Z");
        assert_eq!(parsed.1, 17438);
        assert_eq!(parsed.2, 326);
        assert!(parsed.3);
        assert!(parsed.4.is_none());
    }

    #[test]
    fn parses_codex_rate_limits() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:56.220Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":17438,"output_tokens":326}},"rate_limits":{"primary":{"used_percent":7.0,"window_minutes":300,"resets_at":1771243734},"secondary":{"used_percent":25.0,"window_minutes":10080,"resets_at":1771317088}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex usage");
        assert!(parsed.3);
        let limits = parsed.4.expect("expected limits");
        assert_eq!(limits.primary.expect("primary").window_minutes, 300);
        assert_eq!(limits.secondary.expect("secondary").window_minutes, 10080);
    }

    #[test]
    fn codex_parser_returns_none_without_token_count_or_limits() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:42.927Z","type":"session_meta","payload":{"timestamp":"2026-02-16T09:45:42.927Z"}}
{"timestamp":"2026-02-16T09:45:43.000Z","type":"response_item","payload":{"type":"message"}}"#;
        assert!(parse_codex_session_contents(payload).is_none());
    }

    #[test]
    fn parses_codex_rate_limits_when_info_is_null() {
        let payload = r#"{"timestamp":"2026-02-17T13:47:12.863Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":3.0,"window_minutes":300,"resets_at":1771348283},"secondary":{"used_percent":2.0,"window_minutes":10080,"resets_at":1771922246}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex limits");
        assert_eq!(parsed.0, "2026-02-17T13:47:12.863Z");
        assert!(!parsed.3);
        let limits = parsed.4.expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 3.0);
        assert_eq!(limits.secondary.expect("secondary").used_percent, 2.0);
    }

    #[test]
    fn latest_codex_limits_prefers_newest_session_file() {
        let mut cache = CodexImportCache::default();
        let older = UNIX_EPOCH + Duration::from_secs(100);
        let newer = UNIX_EPOCH + Duration::from_secs(200);

        cache.sessions.insert(
            PathBuf::from("older.jsonl"),
            CachedCodexSession {
                modified: older,
                file_len: 100,
                timestamp: "2026-02-18T00:00:00Z".to_string(),
                input_tokens: 0,
                output_tokens: 0,
                has_token_usage: false,
                limits: Some(CodexRateLimits {
                    timestamp: "2026-02-18T00:00:00Z".to_string(),
                    primary: Some(CodexRateLimit {
                        used_percent: 12.0,
                        window_minutes: 300,
                        resets_at: None,
                    }),
                    secondary: None,
                }),
            },
        );

        cache.sessions.insert(
            PathBuf::from("newer.jsonl"),
            CachedCodexSession {
                modified: newer,
                file_len: 110,
                timestamp: "2026-02-17T23:59:59Z".to_string(),
                input_tokens: 0,
                output_tokens: 0,
                has_token_usage: false,
                limits: Some(CodexRateLimits {
                    timestamp: "2026-02-17T23:59:59Z".to_string(),
                    primary: Some(CodexRateLimit {
                        used_percent: 4.0,
                        window_minutes: 300,
                        resets_at: None,
                    }),
                    secondary: None,
                }),
            },
        );

        let limits = latest_codex_limits(&cache).expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 4.0);
    }
}

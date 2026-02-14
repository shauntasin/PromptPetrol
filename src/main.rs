use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use serde::{Deserialize, Serialize};

const APP_NAME: &str = "PromptPetrol";
const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

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

        Self { api_keys, pricing }
    }
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
struct UsageStats {
    total_input_tokens: u64,
    total_output_tokens: u64,
    total_tokens: u64,
    total_cost_usd: f64,
    budget_usd: Option<f64>,
}

impl UsageStats {
    fn from_data(data: &UsageData) -> Self {
        let total_input_tokens = data.entries.iter().map(|e| e.input_tokens).sum::<u64>();
        let total_output_tokens = data.entries.iter().map(|e| e.output_tokens).sum::<u64>();
        let total_tokens = total_input_tokens + total_output_tokens;
        let total_cost_usd = data.entries.iter().map(|e| e.cost_usd).sum::<f64>();
        Self {
            total_input_tokens,
            total_output_tokens,
            total_tokens,
            total_cost_usd,
            budget_usd: data.budget_usd,
        }
    }

    fn budget_ratio(&self) -> f64 {
        match self.budget_usd {
            Some(budget) if budget > 0.0 => (self.total_cost_usd / budget).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

struct App {
    data_file: PathBuf,
    config_file: PathBuf,
    config: AppConfig,
    data: UsageData,
    status: String,
}

impl App {
    fn new(data_file: PathBuf, config_file: PathBuf) -> Result<Self> {
        let config = load_or_bootstrap_config(&config_file)?;
        let data = load_or_bootstrap_data(&data_file, &config)?;
        Ok(Self {
            data_file,
            config_file,
            config,
            data,
            status: "Ready".to_string(),
        })
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
            Ok(data) => {
                self.data = data;
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
    let stats = UsageStats::from_data(&app.data);
    let area = frame.area();
    let compact_width = area.width < 100;
    let compact_height = area.height < 24;

    let stats_height = if compact_width { 11 } else { 7 };
    let details_min_height = if compact_height { 6 } else { 8 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(stats_height),
            Constraint::Min(details_min_height),
            Constraint::Length(3),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {APP_NAME} "),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Token Usage Dashboard"),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Overview"));
    frame.render_widget(title, chunks[0]);

    let stat_chunks = if compact_width {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Length(4),
                Constraint::Min(3),
            ])
            .split(chunks[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ])
            .split(chunks[1])
    };

    frame.render_widget(
        Paragraph::new(format!(
            "Total Tokens\n{}\n\nInput: {}\nOutput: {}",
            stats.total_tokens, stats.total_input_tokens, stats.total_output_tokens
        ))
        .block(Block::default().borders(Borders::ALL).title("Usage")),
        stat_chunks[0],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "Total Cost\n${:.4}\n\nBudget: {}",
            stats.total_cost_usd,
            stats
                .budget_usd
                .map(|b| format!("${b:.2}"))
                .unwrap_or_else(|| "not set".to_string())
        ))
        .block(Block::default().borders(Borders::ALL).title("Spend")),
        stat_chunks[1],
    );

    let gauge_color = if stats.budget_ratio() >= 0.9 {
        Color::Red
    } else if stats.budget_ratio() >= 0.75 {
        Color::Yellow
    } else {
        Color::Green
    };
    frame.render_widget(
        Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Budget Burn"))
            .ratio(stats.budget_ratio())
            .label(format!("{:.1}%", stats.budget_ratio() * 100.0))
            .gauge_style(Style::default().fg(gauge_color).bg(Color::Black)),
        stat_chunks[2],
    );

    let details = if compact_width || compact_height {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(chunks[2])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(chunks[2])
    };

    let max_rows = max_visible_rows(details[0]);

    let rows = app
        .data
        .entries
        .iter()
        .rev()
        .take(max_rows)
        .map(|e| {
            ListItem::new(format!(
                "{} | {:<10} | {:<18} | {} tok | ${:.4}",
                e.timestamp,
                e.provider,
                e.model,
                e.input_tokens + e.output_tokens,
                e.cost_usd
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(rows).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Recent Activity"),
        ),
        details[0],
    );

    let mut alert_lines = vec![];
    if let Some(budget) = stats.budget_usd {
        let remaining = budget - stats.total_cost_usd;
        if remaining <= 0.0 {
            alert_lines.push("Budget exhausted. Increase limit or reduce usage.".to_string());
        } else {
            alert_lines.push(format!("Remaining budget: ${remaining:.2}"));
        }
    } else {
        alert_lines.push("No budget configured. Set budget_usd in usage.json.".to_string());
    }
    alert_lines.push(format!("Data file: {}", app.data_file.display()));
    alert_lines.push(format!("Config file: {}", app.config_file.display()));

    frame.render_widget(
        Paragraph::new(alert_lines.join("\n"))
            .block(Block::default().borders(Borders::ALL).title("Alerts")),
        details[1],
    );

    let footer = Paragraph::new(format!(
        "q: quit | r: reload | auto: {}s | {}x{} | {}",
        AUTO_REFRESH_INTERVAL.as_secs(),
        area.width,
        area.height,
        app.status
    ))
    .block(Block::default().borders(Borders::ALL).title("Controls"));
    frame.render_widget(footer, chunks[3]);
}

fn max_visible_rows(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(2)).max(1)
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
}

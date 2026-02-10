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
                    provider: "openai".to_string(),
                    model: "gpt-4.1".to_string(),
                    input_tokens: 10_400,
                    output_tokens: 5_800,
                    cost_usd: 0.361,
                },
                UsageEntry {
                    timestamp: "2026-02-10T03:15:00Z".to_string(),
                    provider: "openai".to_string(),
                    model: "gpt-4.1-mini".to_string(),
                    input_tokens: 5_300,
                    output_tokens: 1_200,
                    cost_usd: 0.056,
                },
            ],
        }
    }
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
    data: UsageData,
    status: String,
}

impl App {
    fn new(data_file: PathBuf) -> Result<Self> {
        let data = load_or_bootstrap_data(&data_file)?;
        Ok(Self {
            data_file,
            data,
            status: "Ready".to_string(),
        })
    }

    fn reload(&mut self) {
        match load_or_bootstrap_data(&self.data_file) {
            Ok(data) => {
                self.data = data;
                self.status = format!("Reloaded {}", self.data_file.display());
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
    let mut app = App::new(data_file)?;
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
                "{} | {:<6} | {:<12} | {} tok | ${:.4}",
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

fn default_data_file() -> Result<PathBuf> {
    let base_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("promptpetrol");
    fs::create_dir_all(&base_dir)?;
    Ok(base_dir.join("usage.json"))
}

fn load_or_bootstrap_data(path: &Path) -> Result<UsageData> {
    if path.exists() {
        let contents = fs::read_to_string(path)?;
        let parsed = serde_json::from_str::<UsageData>(&contents)?;
        Ok(parsed)
    } else {
        let seeded = UsageData::default();
        let payload = serde_json::to_string_pretty(&seeded)?;
        fs::write(path, payload)?;
        Ok(seeded)
    }
}

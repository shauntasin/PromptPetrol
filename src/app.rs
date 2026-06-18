use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::DefaultTerminal;

use crate::claude_import::{ClaudeImportDiagnostics, merge_claude_usage};
use crate::codex_import::{CodexImportCache, merge_codex_usage};
use crate::models::{
    AppConfig, UsageData, default_config_file, default_data_file, load_or_bootstrap_config,
    load_or_bootstrap_data,
};
use crate::ui::draw;

/// Live refresh cadence: 10s == 0.1 Hz, the monitoring rate PromptPetrol targets.
pub(crate) const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

pub(crate) struct App {
    pub(crate) config: AppConfig,
    pub(crate) data: UsageData,
    pub(crate) status: String,
    pub(crate) codex_cache: CodexImportCache,
    pub(crate) claude_cache: ClaudeImportDiagnostics,
    pub(crate) show_help: bool,
}

impl App {
    pub(crate) fn new(data_file: PathBuf, config_file: PathBuf) -> Result<Self> {
        let config = load_or_bootstrap_config(&config_file)?;
        let mut data = load_or_bootstrap_data(&data_file, &config)?;
        let mut codex_cache = CodexImportCache::default();
        merge_codex_usage(&mut data, &config, &mut codex_cache);

        let mut claude_cache = ClaudeImportDiagnostics::default();
        merge_claude_usage(&mut data, &config, &mut claude_cache);

        let status = build_status_line(&codex_cache, &claude_cache);
        Ok(Self {
            config,
            data,
            status,
            codex_cache,
            claude_cache,
            show_help: false,
        })
    }

    pub(crate) fn reload(&mut self) {
        if let Ok(path) = default_config_file()
            && let Ok(contents) = std::fs::read_to_string(&path)
            && let Ok(config) = serde_json::from_str::<AppConfig>(&contents)
        {
            self.config = config;
        }

        if let Ok(mut data) = load_or_bootstrap_data_from_config(&self.config) {
            merge_codex_usage(&mut data, &self.config, &mut self.codex_cache);
            merge_claude_usage(&mut data, &self.config, &mut self.claude_cache);
            self.data = data;
        }

        self.status = build_status_line(&self.codex_cache, &self.claude_cache);
    }

    fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }
}

pub(crate) fn run(
    mut terminal: DefaultTerminal,
    app: &mut App,
    refresh_interval: Duration,
) -> Result<()> {
    let mut last_refresh = Instant::now();
    loop {
        terminal.draw(|frame| draw(frame, app))?;

        let elapsed = last_refresh.elapsed();
        let timeout = if elapsed >= refresh_interval {
            Duration::from_millis(0)
        } else {
            refresh_interval - elapsed
        };

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.code == KeyCode::Char('q') => break,
                Event::Key(key) if key.code == KeyCode::Char('r') => {
                    app.reload();
                    last_refresh = Instant::now();
                }
                Event::Key(key) if key.code == KeyCode::Char('?') => {
                    app.toggle_help();
                }
                _ => {}
            }
            continue;
        }

        if last_refresh.elapsed() >= refresh_interval {
            app.reload();
            last_refresh = Instant::now();
        }
    }
    Ok(())
}

pub(crate) fn init_terminal() -> Result<DefaultTerminal> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    Ok(ratatui::init())
}

pub(crate) fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    ratatui::restore();
    Ok(())
}

pub(crate) fn bootstrap_app(
    data_file: Option<PathBuf>,
    config_file: Option<PathBuf>,
) -> Result<App> {
    let data_file = match data_file {
        Some(path) => path,
        None => default_data_file()?,
    };
    let config_file = match config_file {
        Some(path) => path,
        None => default_config_file()?,
    };
    App::new(data_file, config_file)
}

fn load_or_bootstrap_data_from_config(config: &AppConfig) -> Result<UsageData> {
    let path = default_data_file()?;
    load_or_bootstrap_data(&path, config)
}

fn build_status_line(cache: &CodexImportCache, claude_cache: &ClaudeImportDiagnostics) -> String {
    let mut parts = Vec::new();

    if let Some(err) = &claude_cache.fetch_error {
        parts.push(format!("Claude: {err}"));
    } else if claude_cache.limits.is_some() {
        parts.push(format!("Claude: 5h {:.1}%", claude_cache.five_hour_pct));
    }

    let codex_ago = cache
        .diagnostics
        .last_import_at
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if cache.diagnostics.active_files > 0 {
        parts.push(format!("Codex: {}s ago", codex_ago));
    }

    if parts.is_empty() {
        return "Monitoring...".to_string();
    }
    parts.join(" | ")
}

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

use crate::codex_import::{CodexImportCache, codex_import_diagnostics, merge_codex_usage};
use crate::models::{
    AppConfig, UsageData, default_config_file, default_data_file, load_or_bootstrap_config,
    load_or_bootstrap_data, provider_summaries,
};
use crate::ui::draw;

pub(crate) const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

pub(crate) struct App {
    data_file: PathBuf,
    config_file: PathBuf,
    pub(crate) config: AppConfig,
    pub(crate) data: UsageData,
    pub(crate) selected_provider: Option<String>,
    pub(crate) status: String,
    pub(crate) codex_cache: CodexImportCache,
    pub(crate) show_help: bool,
}

impl App {
    pub(crate) fn new(data_file: PathBuf, config_file: PathBuf) -> Result<Self> {
        let config = load_or_bootstrap_config(&config_file)?;
        let mut data = load_or_bootstrap_data(&data_file, &config)?;
        let mut codex_cache = CodexImportCache::default();
        merge_codex_usage(&mut data, &config, &mut codex_cache);
        let status = build_status_line(&config, &codex_cache);
        Ok(Self {
            data_file,
            config_file,
            config,
            data,
            selected_provider: None,
            status,
            codex_cache,
            show_help: false,
        }
        .with_selected_provider())
    }

    pub(crate) fn reload(&mut self) {
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
                self.status = build_status_line(&self.config, &self.codex_cache);
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

        if let Some(selected) = self.selected_provider.as_ref()
            && providers.iter().any(|name| name == selected)
        {
            return;
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

    fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
        self.status = if self.show_help {
            "Help opened".to_string()
        } else {
            "Help closed".to_string()
        };
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

fn build_status_line(config: &AppConfig, cache: &CodexImportCache) -> String {
    if !config.codex_import.enabled {
        return "Ready".to_string();
    }
    let diagnostics = codex_import_diagnostics(cache);
    let imported_ago_secs = diagnostics
        .last_import_at
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "Codex import files:{} refreshed:{} parse_fail:{} scan:{}s updated:{}s",
        diagnostics.active_files,
        diagnostics.refreshed_files,
        diagnostics.parse_failures,
        diagnostics.discovery_interval.as_secs(),
        imported_ago_secs
    )
}

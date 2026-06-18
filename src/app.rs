use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::DefaultTerminal;

use crate::claude_import::{ClaudeImportDiagnostics, merge_claude_usage};
use crate::codex_import::{CodexImportCache, merge_codex_usage};
use crate::models::{AppConfig, default_config_file, load_or_bootstrap_config};
use crate::ui::draw;

/// UI redraw cadence: 500ms == 2 Hz. The render tick is cheap (it only paints
/// cached state), so it can run far faster than the data refresh.
pub(crate) const RENDER_INTERVAL: Duration = Duration::from_millis(500);

/// Data refresh cadence: how often the costly work (network fetch + session
/// scan) runs. Kept slow to avoid spamming the Claude API.
pub(crate) const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

pub(crate) struct App {
    pub(crate) config: AppConfig,
    pub(crate) codex_cache: CodexImportCache,
    pub(crate) claude_cache: ClaudeImportDiagnostics,
    pub(crate) show_help: bool,
}

impl App {
    pub(crate) fn new(config_file: PathBuf) -> Result<Self> {
        let mut app = Self {
            config: load_or_bootstrap_config(&config_file)?,
            codex_cache: CodexImportCache::default(),
            claude_cache: ClaudeImportDiagnostics::default(),
            show_help: false,
        };
        app.reload();
        Ok(app)
    }

    /// Re-reads config from disk and refreshes both data sources. This is the
    /// costly path (network + file scan); the render loop calls it sparingly.
    pub(crate) fn reload(&mut self) {
        if let Ok(path) = default_config_file()
            && let Ok(contents) = std::fs::read_to_string(&path)
            && let Ok(config) = serde_json::from_str::<AppConfig>(&contents)
        {
            self.config = config;
        }
        merge_codex_usage(&self.config, &mut self.codex_cache);
        merge_claude_usage(&self.config, &mut self.claude_cache);
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

        // Wait up to one render tick for input; redraw on timeout for a smooth
        // 2 Hz UI without touching the network or disk every frame.
        if event::poll(RENDER_INTERVAL)?
            && let Event::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('r') => {
                    app.reload();
                    last_refresh = Instant::now();
                }
                KeyCode::Char('?') => app.show_help = !app.show_help,
                _ => {}
            }
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

pub(crate) fn bootstrap_app(config_file: Option<PathBuf>) -> Result<App> {
    let config_file = match config_file {
        Some(path) => path,
        None => default_config_file()?,
    };
    App::new(config_file)
}

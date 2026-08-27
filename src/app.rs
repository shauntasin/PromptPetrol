use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
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
use crate::models::{AppConfig, Theme, default_config_file, load_or_bootstrap_config};
use crate::ui::draw;

/// UI redraw cadence: 500ms == 2 Hz. The render tick is cheap (it only paints
/// cached state), so it can run far faster than the data refresh.
pub(crate) const RENDER_INTERVAL: Duration = Duration::from_millis(500);

/// Data refresh cadence: how often the costly work (network fetch + session
/// scan) runs. Kept slow to avoid spamming the Claude API.
pub(crate) const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

pub(crate) struct App {
    pub(crate) config: AppConfig,
    pub(crate) config_file: PathBuf,
    pub(crate) config_error: Option<String>,
    pub(crate) refresh_error: Option<String>,
    pub(crate) codex_cache: CodexImportCache,
    pub(crate) claude_cache: ClaudeImportDiagnostics,
    pub(crate) show_help: bool,
    theme_override: Option<Theme>,
    refresh_rx: Option<Receiver<RefreshResult>>,
    reload_pending: bool,
}

struct RefreshResult {
    config: AppConfig,
    config_error: Option<String>,
    codex_cache: CodexImportCache,
    claude_cache: ClaudeImportDiagnostics,
}

impl App {
    pub(crate) fn new(config_file: PathBuf) -> Result<Self> {
        let mut app = Self {
            config: load_or_bootstrap_config(&config_file)?,
            config_file,
            config_error: None,
            refresh_error: None,
            codex_cache: CodexImportCache::default(),
            claude_cache: ClaudeImportDiagnostics::default(),
            show_help: false,
            theme_override: None,
            refresh_rx: None,
            reload_pending: false,
        };
        app.reload_now();
        Ok(app)
    }

    /// Re-reads config from disk and refreshes both data sources. This is the
    /// costly path (network + file scan); the render loop calls it sparingly.
    fn reload_now(&mut self) {
        self.reload_config();
        merge_codex_usage(&self.config, &mut self.codex_cache);
        merge_claude_usage(&self.config, &mut self.claude_cache);
    }

    pub(crate) fn request_reload(&mut self) {
        if self.refresh_rx.is_some() {
            self.reload_pending = true;
            return;
        }

        let config_file = self.config_file.clone();
        let mut config = self.config.clone();
        let mut codex_cache = self.codex_cache.clone();
        let mut claude_cache = self.claude_cache.clone();
        let (tx, rx) = mpsc::channel();

        match thread::Builder::new()
            .name("promptpetrol-refresh".into())
            .spawn(move || {
                let config_error = match load_or_bootstrap_config(&config_file) {
                    Ok(reloaded) => {
                        config = reloaded;
                        None
                    }
                    Err(error) => Some(error.to_string()),
                };
                merge_codex_usage(&config, &mut codex_cache);
                merge_claude_usage(&config, &mut claude_cache);
                let _ = tx.send(RefreshResult {
                    config,
                    config_error,
                    codex_cache,
                    claude_cache,
                });
            }) {
            Ok(_) => {
                self.refresh_rx = Some(rx);
                self.reload_pending = false;
                self.refresh_error = None;
            }
            Err(error) => {
                self.refresh_error = Some(format!("failed to start refresh: {error}"));
            }
        }
    }

    pub(crate) fn poll_reload(&mut self) {
        let result = match self.refresh_rx.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => Some(result),
            Some(Err(TryRecvError::Disconnected)) => {
                self.refresh_error = Some("refresh worker stopped unexpectedly".into());
                self.refresh_rx = None;
                None
            }
            Some(Err(TryRecvError::Empty)) | None => return,
        };

        if let Some(result) = result {
            self.config = result.config;
            self.config_error = result.config_error;
            self.codex_cache = result.codex_cache;
            self.claude_cache = result.claude_cache;
            self.refresh_rx = None;
            self.refresh_error = None;
        }

        if self.reload_pending {
            self.request_reload();
        }
    }

    pub(crate) fn is_refreshing(&self) -> bool {
        self.refresh_rx.is_some()
    }

    pub(crate) fn active_theme(&self) -> Theme {
        self.theme_override.unwrap_or(self.config.theme)
    }

    fn cycle_theme(&mut self) {
        self.theme_override = Some(self.active_theme().next());
    }

    #[cfg(test)]
    pub(crate) fn with_test_state(
        config: AppConfig,
        codex_cache: CodexImportCache,
        claude_cache: ClaudeImportDiagnostics,
    ) -> Self {
        Self {
            config,
            config_file: PathBuf::from("test-config.json"),
            config_error: None,
            refresh_error: None,
            codex_cache,
            claude_cache,
            show_help: false,
            theme_override: None,
            refresh_rx: None,
            reload_pending: false,
        }
    }

    fn reload_config(&mut self) {
        match load_or_bootstrap_config(&self.config_file) {
            Ok(config) => {
                self.config = config;
                self.config_error = None;
            }
            Err(error) => self.config_error = Some(error.to_string()),
        }
    }
}

pub(crate) fn run(
    mut terminal: DefaultTerminal,
    app: &mut App,
    refresh_interval: Duration,
) -> Result<()> {
    let mut last_refresh = Instant::now();
    loop {
        app.poll_reload();
        terminal.draw(|frame| draw(frame, app))?;

        // Wait up to one render tick for input; redraw on timeout for a smooth
        // 2 Hz UI without touching the network or disk every frame.
        if event::poll(RENDER_INTERVAL)?
            && let Event::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('r') => {
                    app.request_reload();
                    last_refresh = Instant::now();
                }
                KeyCode::Char('?') => app.show_help = !app.show_help,
                KeyCode::Char('t') => app.cycle_theme(),
                _ => {}
            }
        }

        if last_refresh.elapsed() >= refresh_interval {
            app.request_reload();
            last_refresh = Instant::now();
        }
    }
    Ok(())
}

pub(crate) fn init_terminal() -> Result<DefaultTerminal> {
    enable_raw_mode()?;
    if let Err(error) = execute!(io::stdout(), EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error.into());
    }
    Ok(ratatui::init())
}

pub(crate) fn restore_terminal() -> Result<()> {
    let raw_mode_result = disable_raw_mode();
    let screen_result = execute!(io::stdout(), LeaveAlternateScreen);
    ratatui::restore();
    raw_mode_result?;
    screen_result?;
    Ok(())
}

pub(crate) fn bootstrap_app(config_file: Option<PathBuf>) -> Result<App> {
    let config_file = match config_file {
        Some(path) => path,
        None => default_config_file()?,
    };
    App::new(config_file)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::thread;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn reload_keeps_using_the_selected_config_file() {
        let root = temp_path("custom-config-reload");
        let config_file = root.join("config.json");
        fs::create_dir_all(&root).expect("create test directory");
        fs::write(&config_file, r#"{"codex_import":{"enabled":false}}"#)
            .expect("write initial config");

        let mut app = App {
            config: AppConfig::default(),
            config_file,
            config_error: None,
            refresh_error: None,
            codex_cache: CodexImportCache::default(),
            claude_cache: ClaudeImportDiagnostics::default(),
            show_help: false,
            theme_override: None,
            refresh_rx: None,
            reload_pending: false,
        };
        app.reload_config();
        assert!(!app.config.codex_import.enabled);

        fs::write(&app.config_file, r#"{"codex_import":{"enabled":true}}"#)
            .expect("update custom config");
        app.reload_config();
        assert!(app.config.codex_import.enabled);
        assert!(app.config_error.is_none());

        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn invalid_reload_retains_last_valid_config_and_records_error() {
        let root = temp_path("invalid-config-reload");
        let config_file = root.join("config.json");
        fs::create_dir_all(&root).expect("create test directory");
        fs::write(&config_file, "{}").expect("write initial config");

        let mut app = App {
            config: AppConfig::default(),
            config_file,
            config_error: None,
            refresh_error: None,
            codex_cache: CodexImportCache::default(),
            claude_cache: ClaudeImportDiagnostics::default(),
            show_help: false,
            theme_override: None,
            refresh_rx: None,
            reload_pending: false,
        };
        app.config.codex_import.enabled = false;
        fs::write(&app.config_file, "not json").expect("write invalid config");

        app.reload_config();

        assert!(!app.config.codex_import.enabled);
        assert!(app.config_error.is_some());
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn background_reload_applies_updated_config() {
        let root = temp_path("background-config-reload");
        let config_file = root.join("config.json");
        fs::create_dir_all(&root).expect("create test directory");
        fs::write(
            &config_file,
            r#"{"codex_import":{"enabled":false},"claude_import":{"enabled":false}}"#,
        )
        .expect("write initial config");
        let mut app = App::new(config_file).expect("create app");
        assert!(!app.config.codex_import.enabled);

        fs::write(
            &app.config_file,
            r#"{"codex_import":{"enabled":true,"sessions_dir":"/missing"},"claude_import":{"enabled":false}}"#,
        )
        .expect("update config");
        app.request_reload();
        assert!(app.is_refreshing());

        let deadline = Instant::now() + Duration::from_secs(2);
        while app.is_refreshing() && Instant::now() < deadline {
            app.poll_reload();
            thread::sleep(Duration::from_millis(1));
        }

        assert!(!app.is_refreshing(), "background refresh timed out");
        assert!(app.config.codex_import.enabled);
        assert!(app.config_error.is_none());
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn cycled_theme_survives_config_refreshes() {
        let mut app = App::with_test_state(
            AppConfig::default(),
            CodexImportCache::default(),
            ClaudeImportDiagnostics::default(),
        );

        assert_eq!(app.active_theme(), Theme::Murphy);
        app.cycle_theme();
        assert_eq!(app.active_theme(), Theme::Paper);
        app.config.theme = Theme::Arctic;
        assert_eq!(app.active_theme(), Theme::Paper);
    }

    fn temp_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("promptpetrol-{label}-{nonce}"))
    }
}

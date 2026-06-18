use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::Result;
use serde::{Deserialize, Serialize};

/// Persistent configuration loaded from `config.json`. Unknown fields are
/// ignored, so older configs (which carried `api_keys`/`pricing`) still load.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AppConfig {
    #[serde(default)]
    pub(crate) codex_import: CodexImportConfig,
    #[serde(default)]
    pub(crate) claude_oauth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CodexImportConfig {
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) sessions_dir: Option<String>,
}

impl Default for CodexImportConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sessions_dir: None,
        }
    }
}

fn default_true() -> bool {
    true
}

pub(crate) fn default_config_file() -> Result<PathBuf> {
    Ok(default_config_base_dir()?.join("config.json"))
}

fn default_config_base_dir() -> Result<PathBuf> {
    let base_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("promptpetrol");
    fs::create_dir_all(&base_dir)?;
    Ok(base_dir)
}

pub(crate) fn load_or_bootstrap_config(path: &Path) -> Result<AppConfig> {
    if path.exists() {
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    } else {
        let seeded = AppConfig::default();
        fs::write(path, serde_json::to_string_pretty(&seeded)?)?;
        Ok(seeded)
    }
}

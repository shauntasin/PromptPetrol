use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

/// Persistent configuration loaded from `config.json`. Unknown fields are
/// ignored, so older configs (which carried `api_keys`/`pricing`) still load.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AppConfig {
    #[serde(default)]
    pub(crate) theme: Theme,
    #[serde(default)]
    pub(crate) codex_import: CodexImportConfig,
    #[serde(default)]
    pub(crate) claude_import: ClaudeImportConfig,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) claude_oauth_token: Option<String>,
}

/// Color palette selected for the terminal dashboard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Theme {
    #[default]
    Murphy,
    Paper,
    Arctic,
    #[serde(alias = "solarized")]
    SolarizedLight,
}

impl Theme {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Murphy => Self::Paper,
            Self::Paper => Self::Arctic,
            Self::Arctic => Self::SolarizedLight,
            Self::SolarizedLight => Self::Murphy,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Murphy => "MURPHY",
            Self::Paper => "PAPER",
            Self::Arctic => "ARCTIC",
            Self::SolarizedLight => "SOLARIZED LIGHT",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CodexImportConfig {
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ClaudeImportConfig {
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
}

impl Default for ClaudeImportConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

fn default_true() -> bool {
    true
}

pub(crate) fn default_config_file() -> Result<PathBuf> {
    let base_dir = default_config_base_dir()?;
    fs::create_dir_all(&base_dir)?;
    set_private_directory_permissions(&base_dir)?;
    Ok(base_dir.join("config.json"))
}

fn default_config_base_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .ok_or_else(|| eyre!("cannot determine config directory; use --config-file"))?
        .join("promptpetrol"))
}

pub(crate) fn load_or_bootstrap_config(path: &Path) -> Result<AppConfig> {
    load_config(path).wrap_err_with(|| format!("failed to load config {}", path.display()))
}

fn load_config(path: &Path) -> Result<AppConfig> {
    match File::open(path) {
        Ok(file) => return read_private_config(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
        set_private_directory_permissions(parent)?;
    }
    let seeded = AppConfig::default();
    match write_private_config(path, &serde_json::to_string_pretty(&seeded)?) {
        Ok(()) => Ok(seeded),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            read_private_config(File::open(path)?)
        }
        Err(error) => Err(error.into()),
    }
}

fn write_private_config(path: &Path, contents: &str) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn read_private_config(file: File) -> Result<AppConfig> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(eyre!("config path is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        if mode != 0o600 {
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(serde_json::from_reader(file)?)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn bootstrap_creates_missing_parent_directories() {
        let root = temp_path("nested-config");
        let path = root.join("nested").join("config.json");

        let config = load_or_bootstrap_config(&path).expect("bootstrap config");

        assert_eq!(config.theme, Theme::Murphy);
        assert!(config.codex_import.enabled);
        assert!(path.is_file());
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn themes_deserialize_by_config_name_and_default_to_murphy() {
        let default: AppConfig = serde_json::from_str("{}").expect("default config");
        assert_eq!(default.theme, Theme::Murphy);

        for (name, expected) in [
            ("paper", Theme::Paper),
            ("arctic", Theme::Arctic),
            ("solarized-light", Theme::SolarizedLight),
            ("solarized", Theme::SolarizedLight),
        ] {
            let config: AppConfig =
                serde_json::from_str(&format!(r#"{{"theme":"{name}"}}"#)).expect("named theme");
            assert_eq!(config.theme, expected);
        }
    }

    #[test]
    fn default_config_serializes_without_unset_optional_fields() {
        let value = serde_json::to_value(AppConfig::default()).expect("serialize default config");

        assert!(value.get("claude_oauth_token").is_none());
        assert!(value["codex_import"].get("sessions_dir").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn config_permissions_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_path("private-config");
        let path = root.join("config.json");
        fs::create_dir_all(&root).expect("create test directory");
        fs::write(&path, "{}").expect("seed config");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("set permissions");

        load_or_bootstrap_config(&path).expect("load config");

        let mode = fs::metadata(&path)
            .expect("config metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(root).expect("remove test directory");
    }

    fn temp_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("promptpetrol-{label}-{nonce}"))
    }
}

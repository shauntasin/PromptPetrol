use std::time::SystemTime;

use color_eyre::eyre::{Result, eyre};
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::codex_import::{CodexRateLimit, CodexRateLimits};
use crate::models::AppConfig;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ClaudeOAuthUsage {
    pub(crate) five_hour: ClaudeUsageWindow,
    pub(crate) seven_day: ClaudeUsageWindow,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ClaudeUsageWindow {
    // The OAuth usage endpoint reports `utilization` (already a percent, e.g.
    // 42.0) and `resets_at`. Other fields are ignored by serde.
    pub(crate) resets_at: Option<String>,
    pub(crate) utilization: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ClaudeImportDiagnostics {
    client: Option<Client>,
    pub(crate) last_fetch_at: Option<SystemTime>,
    pub(crate) fetch_error: Option<String>,
    pub(crate) five_hour_pct: f64,
    pub(crate) seven_day_pct: f64,
    pub(crate) limits: Option<CodexRateLimits>,
}

fn build_client() -> Result<Client> {
    Ok(Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()?)
}

fn fetch_claude_usage(
    client: &Client,
    url: &str,
    oauth_token: &str,
) -> Result<Option<ClaudeOAuthUsage>> {
    let response = client
        .get(url)
        .bearer_auth(oauth_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-code/0.1")
        .send()?;

    if response.status() == 401 || response.status() == 403 {
        return Ok(None);
    }

    let response = response.error_for_status()?;
    if !response.status().is_success() {
        return Err(eyre!("unexpected HTTP status: {}", response.status()));
    }
    let usage: ClaudeOAuthUsage = response.json()?;
    validate_usage(&usage)?;
    Ok(Some(usage))
}

fn validate_usage(usage: &ClaudeOAuthUsage) -> Result<()> {
    for value in [usage.five_hour.utilization, usage.seven_day.utilization] {
        if !value.is_finite() || value < 0.0 {
            return Err(eyre!("API returned invalid utilization"));
        }
    }
    Ok(())
}

pub(crate) fn merge_claude_usage(config: &AppConfig, diagnostics: &mut ClaudeImportDiagnostics) {
    if !config.claude_import.enabled {
        *diagnostics = ClaudeImportDiagnostics {
            fetch_error: Some("Disabled".into()),
            ..Default::default()
        };
        return;
    }

    let token = config
        .claude_oauth_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .or_else(detect_claude_token);

    let Some(oauth_token) = token else {
        diagnostics.fetch_error = Some("No OAuth token (set claude_oauth_token in config)".into());
        return;
    };

    // `utilization` from the OAuth endpoint is already a percentage (e.g. 42.0),
    // so it maps straight onto `used_percent`.
    let window = |w: &ClaudeUsageWindow| CodexRateLimit {
        used_percent: w.utilization,
        resets_at: parse_iso_to_epoch(&w.resets_at),
    };

    let client = match diagnostics.client.clone() {
        Some(client) => client,
        None => match build_client() {
            Ok(client) => {
                diagnostics.client = Some(client.clone());
                client
            }
            Err(error) => {
                diagnostics.fetch_error = Some(format!("Client error: {error}"));
                return;
            }
        },
    };

    match fetch_claude_usage(
        &client,
        "https://api.anthropic.com/api/oauth/usage",
        &oauth_token,
    ) {
        Ok(Some(usage)) => {
            diagnostics.last_fetch_at = Some(SystemTime::now());
            diagnostics.fetch_error = None;
            diagnostics.five_hour_pct = usage.five_hour.utilization;
            diagnostics.seven_day_pct = usage.seven_day.utilization;
            diagnostics.limits = Some(CodexRateLimits {
                timestamp: chrono::Utc::now().to_rfc3339(),
                primary: Some(window(&usage.five_hour)),
                secondary: Some(window(&usage.seven_day)),
            });
        }
        Ok(None) => {
            diagnostics.fetch_error = Some("Auth failed (401/403)".into());
        }
        Err(e) => {
            diagnostics.fetch_error = Some(format!("Fetch error: {e}"));
        }
    }
}

#[cfg(target_os = "macos")]
fn detect_claude_token() -> Option<String> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let token = json.get("claudeAiOauth")?.get("accessToken")?.as_str()?;
    let token = token.to_string();
    if token.starts_with("sk-ant-oat") {
        return Some(token);
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn detect_claude_token() -> Option<String> {
    None
}

fn parse_iso_to_epoch(iso: &Option<String>) -> Option<u64> {
    let s = iso.as_deref()?;
    let dt = chrono::DateTime::parse_from_rfc3339(s).ok()?;
    dt.timestamp().try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_import_clears_stale_claude_data() {
        let mut config = AppConfig::default();
        config.claude_import.enabled = false;
        let mut diagnostics = ClaudeImportDiagnostics {
            five_hour_pct: 50.0,
            seven_day_pct: 25.0,
            limits: Some(CodexRateLimits {
                timestamp: "2026-08-19T00:00:00Z".into(),
                primary: Some(CodexRateLimit {
                    used_percent: 50.0,
                    resets_at: None,
                }),
                secondary: None,
            }),
            ..Default::default()
        };

        merge_claude_usage(&config, &mut diagnostics);

        assert_eq!(diagnostics.fetch_error.as_deref(), Some("Disabled"));
        assert!(diagnostics.limits.is_none());
        assert_eq!(diagnostics.five_hour_pct, 0.0);
        assert_eq!(diagnostics.seven_day_pct, 0.0);
    }

    #[test]
    fn reset_timestamps_reject_invalid_and_pre_epoch_values() {
        assert_eq!(parse_iso_to_epoch(&None), None);
        assert_eq!(parse_iso_to_epoch(&Some("not a timestamp".into())), None);
        assert_eq!(
            parse_iso_to_epoch(&Some("1960-01-01T00:00:00Z".into())),
            None
        );
    }
}

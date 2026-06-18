use std::time::SystemTime;

use color_eyre::Result;
use serde::Deserialize;

use crate::codex_import::{CodexRateLimit, CodexRateLimits};
use crate::models::{AppConfig, UsageData};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ClaudeOAuthUsage {
    #[serde(rename = "five_hour")]
    pub(crate) five_hour: ClaudeUsageWindow,
    #[serde(rename = "seven_day")]
    pub(crate) seven_day: ClaudeUsageWindow,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub(crate) struct ClaudeUsageWindow {
    // The OAuth usage endpoint currently reports only `utilization` (already a
    // percent, e.g. 42.0) and `resets_at`. `used`/`limit` are kept optional so a
    // schema that adds them back still deserializes, and one that omits them
    // (today's) does not get rejected.
    #[serde(default)]
    pub(crate) used: Option<u64>,
    #[serde(default)]
    pub(crate) limit: Option<u64>,
    #[serde(rename = "resets_at")]
    pub(crate) resets_at: Option<String>,
    pub(crate) utilization: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ClaudeImportDiagnostics {
    pub(crate) last_fetch_at: Option<SystemTime>,
    pub(crate) fetch_error: Option<String>,
    pub(crate) five_hour_pct: f64,
    pub(crate) seven_day_pct: f64,
    pub(crate) limits: Option<CodexRateLimits>,
}

pub(crate) fn fetch_claude_usage(oauth_token: &str) -> Result<Option<ClaudeOAuthUsage>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let response = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {}", oauth_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-code/0.1")
        .header("Content-Type", "application/json")
        .send()?;

    if response.status() == 401 || response.status() == 403 {
        return Ok(None);
    }

    if !response.status().is_success() {
        return Ok(None);
    }

    let usage: ClaudeOAuthUsage = response.json()?;
    Ok(Some(usage))
}

// `_data` is unused for now: the OAuth endpoint exposes only utilization
// percentages, no token counts to fold into `UsageData`. Kept for signature
// symmetry with `merge_codex_usage` and the `app.rs` reload path.
pub(crate) fn merge_claude_usage(
    _data: &mut UsageData,
    config: &AppConfig,
    diagnostics: &mut ClaudeImportDiagnostics,
) {
    let token = config
        .claude_oauth_token
        .as_ref()
        .filter(|t| !t.is_empty())
        .cloned()
        .or_else(detect_claude_token);

    let Some(oauth_token) = token else {
        diagnostics.fetch_error = Some("No OAuth token (set claude_oauth_token in config)".into());
        return;
    };

    match fetch_claude_usage(&oauth_token) {
        Ok(Some(usage)) => {
            let now = chrono::Utc::now().to_rfc3339();

            // `utilization` from the OAuth endpoint is already a percentage
            // (e.g. 42.0), so it maps straight onto `used_percent`.
            let five_hour_limit = CodexRateLimit {
                used_percent: usage.five_hour.utilization,
                window_minutes: 300,
                resets_at: parse_iso_to_epoch(&usage.five_hour.resets_at),
            };
            let seven_day_limit = CodexRateLimit {
                used_percent: usage.seven_day.utilization,
                window_minutes: 10080,
                resets_at: parse_iso_to_epoch(&usage.seven_day.resets_at),
            };
            let limits = CodexRateLimits {
                timestamp: now,
                primary: Some(five_hour_limit),
                secondary: Some(seven_day_limit),
            };

            diagnostics.last_fetch_at = Some(SystemTime::now());
            diagnostics.fetch_error = None;
            diagnostics.five_hour_pct = usage.five_hour.utilization;
            diagnostics.seven_day_pct = usage.seven_day.utilization;
            diagnostics.limits = Some(limits);
        }
        Ok(None) => {
            diagnostics.fetch_error = Some("Auth failed (401/403)".into());
        }
        Err(e) => {
            diagnostics.fetch_error = Some(format!("Fetch error: {e}"));
        }
    }
}

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

fn parse_iso_to_epoch(iso: &Option<String>) -> Option<u64> {
    let s = iso.as_deref()?;
    let dt = chrono::DateTime::parse_from_rfc3339(s).ok()?;
    Some(dt.timestamp() as u64)
}

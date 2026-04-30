use std::time::SystemTime;

use color_eyre::Result;
use serde::Deserialize;

use crate::models::{AppConfig, UsageData, UsageEntry};

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
    pub(crate) used: u64,
    pub(crate) limit: u64,
    #[serde(rename = "resets_at")]
    pub(crate) resets_at: Option<String>,
    pub(crate) utilization: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ClaudeImportDiagnostics {
    pub(crate) last_fetch_at: Option<SystemTime>,
    pub(crate) fetch_error: Option<String>,
    pub(crate) five_hour_used: u64,
    pub(crate) five_hour_limit: u64,
    pub(crate) seven_day_used: u64,
    pub(crate) seven_day_limit: u64,
}

pub(crate) fn fetch_claude_usage(oauth_token: &str) -> Result<Option<ClaudeOAuthUsage>> {
    let client = reqwest::blocking::Client::new();

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

pub(crate) fn merge_claude_usage(
    data: &mut UsageData,
    config: &AppConfig,
    diagnostics: &mut ClaudeImportDiagnostics,
) {
    let Some(oauth_token) = config.claude_oauth_token.as_ref() else {
        diagnostics.fetch_error = Some("No OAuth token configured".to_string());
        return;
    };

    if oauth_token.is_empty() {
        diagnostics.fetch_error = Some("OAuth token is empty".to_string());
        return;
    }

    match fetch_claude_usage(oauth_token) {
        Ok(Some(usage)) => {
            let now = chrono::Utc::now().to_rfc3339();

            let five_hour_cost = estimate_claude_cost(usage.five_hour.used, "claude-3.7-sonnet");
            let entry = UsageEntry {
                timestamp: now.clone(),
                provider: "anthropic".to_string(),
                model: "claude-pro-subscription".to_string(),
                input_tokens: usage.five_hour.used,
                output_tokens: 0,
                cost_usd: five_hour_cost,
            };

            data.entries.retain(|e| e.provider != "anthropic");
            data.entries.push(entry.clone());
            data.entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

            diagnostics.last_fetch_at = Some(SystemTime::now());
            diagnostics.fetch_error = None;
            diagnostics.five_hour_used = usage.five_hour.used;
            diagnostics.five_hour_limit = usage.five_hour.limit;
            diagnostics.seven_day_used = usage.seven_day.used;
            diagnostics.seven_day_limit = usage.seven_day.limit;
        }
        Ok(None) => {
            diagnostics.fetch_error = Some("Invalid token or not authenticated".to_string());
        }
        Err(e) => {
            diagnostics.fetch_error = Some(format!("Fetch error: {}", e));
        }
    }
}

fn estimate_claude_cost(_tokens: u64, _model: &str) -> f64 {
    0.0
}

#[allow(dead_code)]
pub(crate) fn get_claude_oauth_token_from_cli() -> Option<String> {
    let output = std::process::Command::new("claude")
        .arg("auth")
        .arg("token")
        .output()
        .ok()?;

    if output.status.success() {
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if token.starts_with("sk-ant-oat") {
            return Some(token);
        }
    }
    None
}

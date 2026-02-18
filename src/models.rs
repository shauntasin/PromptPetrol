use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UsageEntry {
    pub(crate) timestamp: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UsageData {
    pub(crate) budget_usd: Option<f64>,
    pub(crate) entries: Vec<UsageEntry>,
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
                    provider: "anthropic".to_string(),
                    model: "claude-3.7-sonnet".to_string(),
                    input_tokens: 10_400,
                    output_tokens: 5_800,
                    cost_usd: 0.361,
                },
                UsageEntry {
                    timestamp: "2026-02-10T03:15:00Z".to_string(),
                    provider: "gemini".to_string(),
                    model: "gemini-2.0-flash".to_string(),
                    input_tokens: 5_300,
                    output_tokens: 1_200,
                    cost_usd: 0.056,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ModelPricing {
    pub(crate) input_per_million_usd: f64,
    pub(crate) output_per_million_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AppConfig {
    #[serde(default)]
    pub(crate) api_keys: HashMap<String, String>,
    #[serde(default)]
    pub(crate) pricing: HashMap<String, ModelPricing>,
    #[serde(default)]
    pub(crate) codex_import: CodexImportConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut api_keys = HashMap::new();
        api_keys.insert("openai".to_string(), "<set-openai-key>".to_string());
        api_keys.insert("anthropic".to_string(), "<set-anthropic-key>".to_string());
        api_keys.insert("gemini".to_string(), "<set-gemini-key>".to_string());
        api_keys.insert("codex".to_string(), "<set-codex-key>".to_string());
        api_keys.insert("opus".to_string(), "<set-opus-key>".to_string());

        let mut pricing = HashMap::new();
        pricing.insert(
            "openai/gpt-4.1-mini".to_string(),
            ModelPricing {
                input_per_million_usd: 0.40,
                output_per_million_usd: 1.60,
            },
        );
        pricing.insert(
            "anthropic/claude-3.7-sonnet".to_string(),
            ModelPricing {
                input_per_million_usd: 3.00,
                output_per_million_usd: 15.00,
            },
        );
        pricing.insert(
            "gemini/gemini-2.0-flash".to_string(),
            ModelPricing {
                input_per_million_usd: 0.35,
                output_per_million_usd: 1.05,
            },
        );

        Self {
            api_keys,
            pricing,
            codex_import: CodexImportConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CodexImportConfig {
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) sessions_dir: Option<String>,
    #[serde(default = "default_codex_model")]
    pub(crate) model: String,
}

impl Default for CodexImportConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sessions_dir: None,
            model: default_codex_model(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_codex_model() -> String {
    "codex-cli".to_string()
}

#[derive(Debug, Clone, Deserialize)]
struct RawUsageData {
    budget_usd: Option<f64>,
    entries: Vec<RawUsageEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawUsageEntry {
    timestamp: String,
    provider: String,
    model: String,
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    request_tokens: Option<u64>,
    #[serde(default)]
    response_tokens: Option<u64>,
    #[serde(default)]
    prompt_token_count: Option<u64>,
    #[serde(default)]
    candidates_token_count: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
    #[serde(default)]
    total_token_count: Option<u64>,
    #[serde(default)]
    cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderSummary {
    pub(crate) provider: String,
    pub(crate) total_tokens: u64,
    pub(crate) total_cost_usd: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderStats {
    pub(crate) provider: String,
    pub(crate) total_tokens: u64,
    pub(crate) total_cost_usd: f64,
    pub(crate) requests: usize,
}

pub(crate) fn provider_summaries(data: &UsageData) -> Vec<ProviderSummary> {
    let mut grouped: HashMap<String, (u64, f64)> = HashMap::new();
    for entry in &data.entries {
        let current = grouped.entry(entry.provider.clone()).or_insert((0, 0.0));
        current.0 += entry.input_tokens + entry.output_tokens;
        current.1 += entry.cost_usd;
    }

    let mut summaries = grouped
        .into_iter()
        .map(
            |(provider, (total_tokens, total_cost_usd))| ProviderSummary {
                provider,
                total_tokens,
                total_cost_usd,
            },
        )
        .collect::<Vec<_>>();
    summaries.sort_by(|a, b| {
        b.total_cost_usd
            .partial_cmp(&a.total_cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.total_tokens.cmp(&a.total_tokens))
            .then_with(|| a.provider.cmp(&b.provider))
    });
    summaries
}

pub(crate) fn provider_stats(data: &UsageData, provider: &str) -> Option<ProviderStats> {
    if provider.is_empty() {
        return None;
    }

    let mut total_input_tokens = 0_u64;
    let mut total_output_tokens = 0_u64;
    let mut total_cost_usd = 0.0_f64;
    let mut requests = 0_usize;

    for entry in &data.entries {
        if entry.provider != provider {
            continue;
        }
        total_input_tokens += entry.input_tokens;
        total_output_tokens += entry.output_tokens;
        total_cost_usd += entry.cost_usd;
        requests += 1;
    }

    if requests == 0 {
        return None;
    }

    Some(ProviderStats {
        provider: provider.to_string(),
        total_tokens: total_input_tokens + total_output_tokens,
        total_cost_usd,
        requests,
    })
}

pub(crate) fn default_data_file() -> Result<PathBuf> {
    Ok(default_config_base_dir()?.join("usage.json"))
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
        let contents = fs::read_to_string(path)?;
        let parsed = serde_json::from_str::<AppConfig>(&contents)?;
        Ok(parsed)
    } else {
        let seeded = AppConfig::default();
        let payload = serde_json::to_string_pretty(&seeded)?;
        fs::write(path, payload)?;
        Ok(seeded)
    }
}

pub(crate) fn load_or_bootstrap_data(path: &Path, config: &AppConfig) -> Result<UsageData> {
    if path.exists() {
        let contents = fs::read_to_string(path)?;
        if let Ok(parsed) = serde_json::from_str::<UsageData>(&contents) {
            return Ok(parsed);
        }

        let raw = serde_json::from_str::<RawUsageData>(&contents)?;
        Ok(normalize_raw_usage(raw, config))
    } else {
        let seeded = UsageData::default();
        let payload = serde_json::to_string_pretty(&seeded)?;
        fs::write(path, payload)?;
        Ok(seeded)
    }
}

fn normalize_raw_usage(raw: RawUsageData, config: &AppConfig) -> UsageData {
    let entries = raw
        .entries
        .into_iter()
        .map(|entry| normalize_entry(entry, config))
        .collect::<Vec<_>>();

    UsageData {
        budget_usd: raw.budget_usd,
        entries,
    }
}

fn normalize_entry(raw: RawUsageEntry, config: &AppConfig) -> UsageEntry {
    let provider = raw.provider.to_lowercase();
    let (input_tokens, output_tokens) = match provider.as_str() {
        "openai" => adapt_openai_tokens(&raw),
        "codex" => adapt_codex_tokens(&raw),
        "anthropic" => adapt_anthropic_tokens(&raw),
        "gemini" => adapt_gemini_tokens(&raw),
        "opus" => adapt_opus_tokens(&raw),
        _ => adapt_generic_tokens(&raw),
    };

    let cost_usd = raw.cost_usd.unwrap_or_else(|| {
        estimate_cost_usd(
            &provider,
            &raw.model,
            input_tokens,
            output_tokens,
            &config.pricing,
        )
    });

    UsageEntry {
        timestamp: raw.timestamp,
        provider,
        model: raw.model,
        input_tokens,
        output_tokens,
        cost_usd,
    }
}

fn adapt_openai_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    let input = raw
        .input_tokens
        .or(raw.prompt_tokens)
        .or(raw.request_tokens)
        .unwrap_or(0);
    let output = raw
        .output_tokens
        .or(raw.completion_tokens)
        .or(raw.response_tokens)
        .unwrap_or(0);
    split_with_total(input, output, raw.total_tokens)
}

fn adapt_codex_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    adapt_openai_tokens(raw)
}

fn adapt_anthropic_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    let input = raw
        .input_tokens
        .or(raw.prompt_tokens)
        .or(raw.request_tokens)
        .unwrap_or(0);
    let output = raw
        .output_tokens
        .or(raw.completion_tokens)
        .or(raw.response_tokens)
        .unwrap_or(0);
    split_with_total(input, output, raw.total_tokens)
}

fn adapt_gemini_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    let input = raw
        .input_tokens
        .or(raw.prompt_token_count)
        .or(raw.prompt_tokens)
        .unwrap_or(0);
    let output = raw
        .output_tokens
        .or(raw.candidates_token_count)
        .or(raw.completion_tokens)
        .unwrap_or(0);
    split_with_total(input, output, raw.total_tokens.or(raw.total_token_count))
}

fn adapt_opus_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    let input = raw
        .input_tokens
        .or(raw.prompt_tokens)
        .or(raw.prompt_token_count)
        .unwrap_or(0);
    let output = raw
        .output_tokens
        .or(raw.completion_tokens)
        .or(raw.candidates_token_count)
        .unwrap_or(0);
    split_with_total(input, output, raw.total_tokens.or(raw.total_token_count))
}

fn adapt_generic_tokens(raw: &RawUsageEntry) -> (u64, u64) {
    let input = raw
        .input_tokens
        .or(raw.prompt_tokens)
        .or(raw.request_tokens)
        .or(raw.prompt_token_count)
        .unwrap_or(0);
    let output = raw
        .output_tokens
        .or(raw.completion_tokens)
        .or(raw.response_tokens)
        .or(raw.candidates_token_count)
        .unwrap_or(0);
    split_with_total(input, output, raw.total_tokens.or(raw.total_token_count))
}

fn split_with_total(input: u64, output: u64, total: Option<u64>) -> (u64, u64) {
    if input == 0 && output == 0 {
        if let Some(total) = total {
            let input_guess = total / 2;
            return (input_guess, total - input_guess);
        }
    }

    if let Some(total) = total {
        let known = input + output;
        if known == 0 {
            let input_guess = total / 2;
            return (input_guess, total - input_guess);
        }
        if known < total {
            return (input, output + (total - known));
        }
    }

    (input, output)
}

pub(crate) fn estimate_cost_usd(
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    pricing: &HashMap<String, ModelPricing>,
) -> f64 {
    if let Some(model_pricing) = lookup_pricing(pricing, provider, model) {
        return (input_tokens as f64 / 1_000_000.0) * model_pricing.input_per_million_usd
            + (output_tokens as f64 / 1_000_000.0) * model_pricing.output_per_million_usd;
    }

    0.0
}

fn lookup_pricing<'a>(
    pricing: &'a HashMap<String, ModelPricing>,
    provider: &str,
    model: &str,
) -> Option<&'a ModelPricing> {
    let exact = format!("{provider}/{model}");
    if let Some(found) = pricing.get(&exact) {
        return Some(found);
    }

    let wildcard = format!("{provider}/*");
    pricing.get(&wildcard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_openai_entry() {
        let raw = RawUsageData {
            budget_usd: Some(25.0),
            entries: vec![RawUsageEntry {
                timestamp: "2026-02-10T03:15:00Z".to_string(),
                provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
                input_tokens: None,
                output_tokens: None,
                prompt_tokens: Some(1200),
                completion_tokens: Some(300),
                request_tokens: None,
                response_tokens: None,
                prompt_token_count: None,
                candidates_token_count: None,
                total_tokens: None,
                total_token_count: None,
                cost_usd: None,
            }],
        };

        let normalized = normalize_raw_usage(raw, &AppConfig::default());
        assert_eq!(normalized.entries[0].input_tokens, 1200);
        assert_eq!(normalized.entries[0].output_tokens, 300);
        assert!(normalized.entries[0].cost_usd > 0.0);
    }

    #[test]
    fn normalizes_gemini_total_only() {
        let raw = RawUsageData {
            budget_usd: Some(25.0),
            entries: vec![RawUsageEntry {
                timestamp: "2026-02-10T03:15:00Z".to_string(),
                provider: "gemini".to_string(),
                model: "gemini-2.0-flash".to_string(),
                input_tokens: None,
                output_tokens: None,
                prompt_tokens: None,
                completion_tokens: None,
                request_tokens: None,
                response_tokens: None,
                prompt_token_count: None,
                candidates_token_count: None,
                total_tokens: None,
                total_token_count: Some(1000),
                cost_usd: None,
            }],
        };

        let normalized = normalize_raw_usage(raw, &AppConfig::default());
        assert_eq!(normalized.entries[0].input_tokens, 500);
        assert_eq!(normalized.entries[0].output_tokens, 500);
    }
}

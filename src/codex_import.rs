use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::models::{AppConfig, UsageData, UsageEntry, estimate_cost_usd};

#[derive(Debug, Clone)]
struct CachedCodexSession {
    modified: SystemTime,
    file_len: u64,
    timestamp: String,
    input_tokens: u64,
    output_tokens: u64,
    has_token_usage: bool,
    limits: Option<CodexRateLimits>,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexRateLimit {
    pub(crate) used_percent: f64,
    pub(crate) window_minutes: u64,
    pub(crate) resets_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexRateLimits {
    timestamp: String,
    pub(crate) primary: Option<CodexRateLimit>,
    pub(crate) secondary: Option<CodexRateLimit>,
}

#[derive(Debug, Default)]
pub(crate) struct CodexImportCache {
    sessions: HashMap<PathBuf, CachedCodexSession>,
}

pub(crate) fn merge_codex_usage(data: &mut UsageData, config: &AppConfig, cache: &mut CodexImportCache) {
    if !config.codex_import.enabled {
        return;
    }

    let sessions_dir = codex_sessions_dir(config);
    let Some(session_files) = collect_codex_session_files(&sessions_dir) else {
        return;
    };

    let mut active = HashSet::new();
    for file in session_files {
        active.insert(file.clone());
        let (modified, file_len) = match fs::metadata(&file) {
            Ok(metadata) => match metadata.modified() {
                Ok(modified) => (modified, metadata.len()),
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        let needs_refresh = cache
            .sessions
            .get(&file)
            .map(|cached| cached.modified != modified || cached.file_len != file_len)
            .unwrap_or(true);

        if !needs_refresh {
            continue;
        }

        if let Some(parsed) = parse_codex_session_file(&file, modified, file_len) {
            cache.sessions.insert(file, parsed);
        } else {
            cache.sessions.remove(&file);
        }
    }

    cache.sessions.retain(|path, _| active.contains(path));

    let mut imported = cache
        .sessions
        .values()
        .filter(|session| session.has_token_usage)
        .map(|session| {
            let model = &config.codex_import.model;
            UsageEntry {
                timestamp: session.timestamp.clone(),
                provider: "codex".to_string(),
                model: model.clone(),
                input_tokens: session.input_tokens,
                output_tokens: session.output_tokens,
                cost_usd: estimate_cost_usd(
                    "codex",
                    model,
                    session.input_tokens,
                    session.output_tokens,
                    &config.pricing,
                ),
            }
        })
        .collect::<Vec<_>>();

    data.entries.append(&mut imported);
    data.entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
}

pub(crate) fn latest_codex_limits(cache: &CodexImportCache) -> Option<CodexRateLimits> {
    cache
        .sessions
        .values()
        .filter_map(|session| {
            session
                .limits
                .as_ref()
                .map(|limits| (session.modified, &limits.timestamp, limits))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)))
        .map(|(_, _, limits)| limits.clone())
}

fn codex_sessions_dir(config: &AppConfig) -> PathBuf {
    if let Some(path) = config.codex_import.sessions_dir.as_ref() {
        return PathBuf::from(path);
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
        .join("sessions")
}

fn collect_codex_session_files(dir: &Path) -> Option<Vec<PathBuf>> {
    if !dir.exists() {
        return None;
    }

    let mut files = Vec::new();
    collect_jsonl_files_recursive(dir, &mut files).ok()?;
    Some(files)
}

fn collect_jsonl_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files_recursive(&path, files)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
    Ok(())
}

fn parse_codex_session_file(
    path: &Path,
    modified: SystemTime,
    file_len: u64,
) -> Option<CachedCodexSession> {
    let contents = fs::read_to_string(path).ok()?;
    let (timestamp, input_tokens, output_tokens, has_token_usage, limits) =
        parse_codex_session_contents(&contents)?;
    Some(CachedCodexSession {
        modified,
        file_len,
        timestamp,
        input_tokens,
        output_tokens,
        has_token_usage,
        limits,
    })
}

fn parse_codex_session_contents(
    contents: &str,
) -> Option<(String, u64, u64, bool, Option<CodexRateLimits>)> {
    let mut session_timestamp: Option<String> = None;
    let mut latest_event_timestamp: Option<String> = None;
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut has_token_usage = false;
    let mut latest_limits: Option<CodexRateLimits> = None;

    for line in contents.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if v.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(ts) = v.pointer("/payload/timestamp").and_then(Value::as_str) {
                session_timestamp = Some(ts.to_string());
            }
        }

        let is_token_count = v.get("type").and_then(Value::as_str) == Some("event_msg")
            && v.pointer("/payload/type").and_then(Value::as_str) == Some("token_count");

        if !is_token_count {
            continue;
        }

        let event_timestamp = v
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(ts) = event_timestamp.as_ref() {
            latest_event_timestamp = Some(ts.clone());
            let primary = parse_codex_rate_limit(v.pointer("/payload/rate_limits/primary"));
            let secondary = parse_codex_rate_limit(v.pointer("/payload/rate_limits/secondary"));
            if primary.is_some() || secondary.is_some() {
                latest_limits = Some(CodexRateLimits {
                    timestamp: ts.clone(),
                    primary,
                    secondary,
                });
            }
        }

        let maybe_input = v
            .pointer("/payload/info/total_token_usage/input_tokens")
            .and_then(Value::as_u64);
        let maybe_output = v
            .pointer("/payload/info/total_token_usage/output_tokens")
            .and_then(Value::as_u64);

        if let (Some(input), Some(output)) = (maybe_input, maybe_output) {
            input_tokens = input;
            output_tokens = output;
            has_token_usage = true;
        }
    }

    let timestamp = latest_event_timestamp.or(session_timestamp)?;
    if !has_token_usage && latest_limits.is_none() {
        return None;
    }
    Some((
        timestamp,
        input_tokens,
        output_tokens,
        has_token_usage,
        latest_limits,
    ))
}

fn parse_codex_rate_limit(node: Option<&Value>) -> Option<CodexRateLimit> {
    let node = node?;
    let used_percent = node
        .get("used_percent")
        .and_then(Value::as_f64)
        .or_else(|| {
            node.get("used_percent")
                .and_then(Value::as_u64)
                .map(|v| v as f64)
        })?;
    let window_minutes = node.get("window_minutes").and_then(Value::as_u64)?;
    let resets_at = node.get("resets_at").and_then(Value::as_u64);
    Some(CodexRateLimit {
        used_percent,
        window_minutes,
        resets_at,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::models::{AppConfig, UsageData};

    #[test]
    fn parses_codex_session_usage_from_token_count_events() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:42.927Z","type":"session_meta","payload":{"timestamp":"2026-02-16T09:45:42.927Z"}}
{"timestamp":"2026-02-16T09:45:53.237Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":8582,"output_tokens":210}}}}
{"timestamp":"2026-02-16T09:45:56.220Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":17438,"output_tokens":326}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex usage");
        assert_eq!(parsed.0, "2026-02-16T09:45:56.220Z");
        assert_eq!(parsed.1, 17438);
        assert_eq!(parsed.2, 326);
        assert!(parsed.3);
        assert!(parsed.4.is_none());
    }

    #[test]
    fn parses_codex_rate_limits() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:56.220Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":17438,"output_tokens":326}},"rate_limits":{"primary":{"used_percent":7.0,"window_minutes":300,"resets_at":1771243734},"secondary":{"used_percent":25.0,"window_minutes":10080,"resets_at":1771317088}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex usage");
        assert!(parsed.3);
        let limits = parsed.4.expect("expected limits");
        assert_eq!(limits.primary.expect("primary").window_minutes, 300);
        assert_eq!(limits.secondary.expect("secondary").window_minutes, 10080);
    }

    #[test]
    fn codex_parser_returns_none_without_token_count_or_limits() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:42.927Z","type":"session_meta","payload":{"timestamp":"2026-02-16T09:45:42.927Z"}}
{"timestamp":"2026-02-16T09:45:43.000Z","type":"response_item","payload":{"type":"message"}}"#;
        assert!(parse_codex_session_contents(payload).is_none());
    }

    #[test]
    fn parses_codex_rate_limits_when_info_is_null() {
        let payload = r#"{"timestamp":"2026-02-17T13:47:12.863Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":3.0,"window_minutes":300,"resets_at":1771348283},"secondary":{"used_percent":2.0,"window_minutes":10080,"resets_at":1771922246}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex limits");
        assert_eq!(parsed.0, "2026-02-17T13:47:12.863Z");
        assert!(!parsed.3);
        let limits = parsed.4.expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 3.0);
        assert_eq!(limits.secondary.expect("secondary").used_percent, 2.0);
    }

    #[test]
    fn latest_codex_limits_prefers_newest_session_file() {
        let mut cache = CodexImportCache::default();
        let older = UNIX_EPOCH + Duration::from_secs(100);
        let newer = UNIX_EPOCH + Duration::from_secs(200);

        cache.sessions.insert(
            PathBuf::from("older.jsonl"),
            CachedCodexSession {
                modified: older,
                file_len: 100,
                timestamp: "2026-02-18T00:00:00Z".to_string(),
                input_tokens: 0,
                output_tokens: 0,
                has_token_usage: false,
                limits: Some(CodexRateLimits {
                    timestamp: "2026-02-18T00:00:00Z".to_string(),
                    primary: Some(CodexRateLimit {
                        used_percent: 12.0,
                        window_minutes: 300,
                        resets_at: None,
                    }),
                    secondary: None,
                }),
            },
        );

        cache.sessions.insert(
            PathBuf::from("newer.jsonl"),
            CachedCodexSession {
                modified: newer,
                file_len: 110,
                timestamp: "2026-02-17T23:59:59Z".to_string(),
                input_tokens: 0,
                output_tokens: 0,
                has_token_usage: false,
                limits: Some(CodexRateLimits {
                    timestamp: "2026-02-17T23:59:59Z".to_string(),
                    primary: Some(CodexRateLimit {
                        used_percent: 4.0,
                        window_minutes: 300,
                        resets_at: None,
                    }),
                    secondary: None,
                }),
            },
        );

        let limits = latest_codex_limits(&cache).expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 4.0);
    }

    #[test]
    fn parses_fixture_with_malformed_and_mixed_events() {
        let payload = fixture_contents("mixed_usage_and_limits.jsonl");
        let parsed = parse_codex_session_contents(&payload).expect("expected parsed fixture");
        assert_eq!(parsed.0, "2026-02-18T10:01:10.000Z");
        assert_eq!(parsed.1, 180);
        assert_eq!(parsed.2, 55);
        assert!(parsed.3);
        let limits = parsed.4.expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 5.0);
        assert_eq!(limits.secondary.expect("secondary").used_percent, 3.0);
    }

    #[test]
    fn merge_codex_usage_uses_fixture_sessions_and_ignores_invalid_files() {
        let temp_root = make_temp_dir("codex-fixtures");
        let session_dir = temp_root.join("2026").join("02").join("18");
        fs::create_dir_all(&session_dir).expect("create session dir");

        write_fixture(&session_dir, "mixed_usage_and_limits.jsonl");
        write_fixture(&session_dir, "limits_only_malformed.jsonl");
        write_fixture(&session_dir, "no_token_or_limits_mixed.jsonl");

        let mut config = AppConfig::default();
        config.codex_import.enabled = true;
        config.codex_import.sessions_dir = Some(temp_root.to_string_lossy().to_string());
        config.codex_import.model = "codex-cli".to_string();

        let mut data = UsageData {
            budget_usd: Some(10.0),
            entries: vec![],
        };
        let mut cache = CodexImportCache::default();
        merge_codex_usage(&mut data, &config, &mut cache);

        let codex_entries = data
            .entries
            .iter()
            .filter(|entry| entry.provider == "codex")
            .collect::<Vec<_>>();
        assert_eq!(codex_entries.len(), 1);
        assert_eq!(codex_entries[0].input_tokens, 180);
        assert_eq!(codex_entries[0].output_tokens, 55);
        assert_eq!(codex_entries[0].timestamp, "2026-02-18T10:01:10.000Z");

        let limits = latest_codex_limits(&cache).expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 9.0);
        assert_eq!(limits.secondary.expect("secondary").used_percent, 4.0);

        let _ = fs::remove_dir_all(temp_root);
    }

    fn fixture_contents(name: &str) -> String {
        fs::read_to_string(fixture_path(name)).expect("read fixture file")
    }

    fn write_fixture(target_dir: &Path, fixture_name: &str) {
        let contents = fixture_contents(fixture_name);
        let target = target_dir.join(fixture_name);
        fs::write(target, contents).expect("write fixture");
    }

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("codex")
            .join(name)
    }

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("promptpetrol-{prefix}-{nanos}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }
}

use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

use crate::models::AppConfig;

const MIN_DISCOVERY_INTERVAL: Duration = Duration::from_secs(10);
const MAX_DISCOVERY_INTERVAL: Duration = Duration::from_secs(120);
const DISCOVERY_BACKOFF_STEP: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
struct CachedCodexSession {
    modified: SystemTime,
    file_len: u64,
    timestamp: String,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    context_window: u64,
    has_token_usage: bool,
    limits: Option<CodexRateLimits>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct CodexImportDiagnostics {
    pub(crate) active_files: usize,
    pub(crate) refreshed_files: usize,
    pub(crate) parse_error_files: usize,
    pub(crate) no_usage_or_limits_files: usize,
    pub(crate) unreadable_files: usize,
    pub(crate) last_import_at: Option<SystemTime>,
    pub(crate) discovery_interval: Duration,
}

impl Default for CodexImportDiagnostics {
    fn default() -> Self {
        Self {
            active_files: 0,
            refreshed_files: 0,
            parse_error_files: 0,
            no_usage_or_limits_files: 0,
            unreadable_files: 0,
            last_import_at: None,
            discovery_interval: MIN_DISCOVERY_INTERVAL,
        }
    }
}

enum ParsedSessionFile {
    Parsed(CachedCodexSession),
    NoUsageOrLimits,
    ParseError,
    Unreadable,
}

enum ParsedSessionContents {
    Parsed(CodexSessionData),
    NoUsageOrLimits,
    ParseError,
}

struct CodexSessionData {
    timestamp: String,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    context_window: u64,
    has_token_usage: bool,
    limits: Option<CodexRateLimits>,
}

#[derive(Debug, Deserialize)]
struct CodexSessionLine {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    payload: Option<CodexSessionLinePayload>,
}

#[derive(Debug, Deserialize)]
struct CodexSessionLinePayload {
    #[serde(rename = "type", default)]
    payload_type: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    info: Option<CodexTokenInfo>,
    #[serde(default)]
    rate_limits: Option<CodexEventRateLimits>,
}

#[derive(Debug, Deserialize)]
struct CodexTokenInfo {
    #[serde(default)]
    total_token_usage: Option<CodexTotalTokenUsage>,
    #[serde(default)]
    model_context_window: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CodexTotalTokenUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct CodexEventRateLimits {
    #[serde(default)]
    primary: Option<CodexRawRateLimit>,
    #[serde(default)]
    secondary: Option<CodexRawRateLimit>,
}

#[derive(Debug, Deserialize)]
struct CodexRawRateLimit {
    used_percent: CodexRateLimitPercent,
    #[serde(default)]
    resets_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CodexRateLimitPercent {
    Float(f64),
    Int(u64),
}

impl CodexRateLimitPercent {
    fn as_f64(&self) -> f64 {
        match self {
            Self::Float(value) => *value,
            Self::Int(value) => *value as f64,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CodexRateLimit {
    pub(crate) used_percent: f64,
    pub(crate) resets_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexRateLimits {
    pub(crate) timestamp: String,
    pub(crate) primary: Option<CodexRateLimit>,
    pub(crate) secondary: Option<CodexRateLimit>,
}

#[derive(Debug)]
pub(crate) struct CodexImportCache {
    sessions: HashMap<PathBuf, CachedCodexSession>,
    pub(crate) latest_limits: Option<CodexRateLimits>,
    session_files: Vec<PathBuf>,
    last_discovery_at: Option<SystemTime>,
    session_discovery_interval: Duration,
    idle_discovery_cycles: u32,
    pub(crate) diagnostics: CodexImportDiagnostics,
}

impl Default for CodexImportCache {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
            latest_limits: None,
            session_files: Vec::new(),
            last_discovery_at: None,
            session_discovery_interval: MIN_DISCOVERY_INTERVAL,
            idle_discovery_cycles: 0,
            diagnostics: CodexImportDiagnostics::default(),
        }
    }
}

#[cfg(test)]
impl CodexImportCache {
    /// Test helper: seed a single session so `codex_session_snapshot` reports a
    /// context window, without touching the filesystem.
    pub(crate) fn with_test_context(input: u64, cached: u64, output: u64, window: u64) -> Self {
        let mut cache = Self::default();
        cache.sessions.insert(
            PathBuf::from("test-session.jsonl"),
            CachedCodexSession {
                modified: SystemTime::now(),
                file_len: 0,
                timestamp: "2026-06-18T00:00:00Z".to_string(),
                input_tokens: input,
                output_tokens: output,
                cached_input_tokens: cached,
                context_window: window,
                has_token_usage: true,
                limits: None,
            },
        );
        cache
    }
}

pub(crate) fn merge_codex_usage(config: &AppConfig, cache: &mut CodexImportCache) {
    if !config.codex_import.enabled {
        return;
    }

    let sessions_dir = codex_sessions_dir(config);
    let mut changes_detected = false;
    let mut discovery_ran = false;
    if should_refresh_file_discovery(cache) {
        discovery_ran = true;
        let previous_count = cache.session_files.len();
        cache.session_files = collect_codex_session_files(&sessions_dir).unwrap_or_default();
        cache.last_discovery_at = Some(SystemTime::now());
        changes_detected = changes_detected || cache.session_files.len() != previous_count;
    }

    // `session_files` is the authoritative active set; only refresh entries whose
    // mtime/len changed, and drop cached sessions whose file is gone or invalid.
    let mut refreshed_files = 0_usize;
    let mut parse_error_files = 0_usize;
    let mut no_usage_or_limits_files = 0_usize;
    let mut unreadable_files = 0_usize;
    let files = std::mem::take(&mut cache.session_files);
    for file in &files {
        let (modified, file_len) =
            match fs::metadata(file).and_then(|m| Ok((m.modified()?, m.len()))) {
                Ok(meta) => meta,
                Err(_) => {
                    changes_detected = true;
                    unreadable_files += 1;
                    cache.sessions.remove(file);
                    continue;
                }
            };

        let needs_refresh = cache
            .sessions
            .get(file)
            .is_none_or(|cached| cached.modified != modified || cached.file_len != file_len);
        if !needs_refresh {
            continue;
        }
        changes_detected = true;
        refreshed_files += 1;

        match parse_codex_session_file(file, modified, file_len) {
            ParsedSessionFile::Parsed(parsed) => {
                cache.sessions.insert(file.clone(), parsed);
            }
            ParsedSessionFile::NoUsageOrLimits => {
                no_usage_or_limits_files += 1;
                cache.sessions.remove(file);
            }
            ParsedSessionFile::ParseError => {
                parse_error_files += 1;
                cache.sessions.remove(file);
            }
            ParsedSessionFile::Unreadable => {
                unreadable_files += 1;
                cache.sessions.remove(file);
            }
        }
    }

    // Drop any cached session whose file is no longer discovered.
    cache.sessions.retain(|path, _| files.contains(path));
    let active_count = files.len();
    cache.session_files = files;
    cache.latest_limits = find_latest_limits(&cache.sessions);
    if discovery_ran {
        tune_discovery_interval(cache, changes_detected);
    }
    cache.diagnostics = CodexImportDiagnostics {
        active_files: active_count,
        refreshed_files,
        parse_error_files,
        no_usage_or_limits_files,
        unreadable_files,
        last_import_at: Some(SystemTime::now()),
        discovery_interval: cache.session_discovery_interval,
    };
}

fn should_refresh_file_discovery(cache: &CodexImportCache) -> bool {
    let Some(last_discovery) = cache.last_discovery_at else {
        return true;
    };
    match SystemTime::now().duration_since(last_discovery) {
        Ok(elapsed) => elapsed >= cache.session_discovery_interval,
        Err(_) => true,
    }
}

fn tune_discovery_interval(cache: &mut CodexImportCache, changes_detected: bool) {
    if changes_detected {
        cache.session_discovery_interval = MIN_DISCOVERY_INTERVAL;
        cache.idle_discovery_cycles = 0;
        return;
    }

    cache.idle_discovery_cycles += 1;
    if cache.idle_discovery_cycles < 3 {
        return;
    }

    cache.idle_discovery_cycles = 0;
    let next = cache.session_discovery_interval + DISCOVERY_BACKOFF_STEP;
    cache.session_discovery_interval = std::cmp::min(next, MAX_DISCOVERY_INTERVAL);
}

/// The most-recent session's token figures, used for the context-window gauge.
/// Context is a per-conversation measure, so only the newest session matters.
pub(crate) struct CodexSessionSnapshot {
    pub(crate) latest_input: u64,
    pub(crate) latest_output: u64,
    pub(crate) latest_cached: u64,
    pub(crate) latest_context_window: u64,
    #[cfg(test)]
    pub(crate) latest_timestamp: Option<String>,
}

pub(crate) fn codex_session_snapshot(cache: &CodexImportCache) -> Option<CodexSessionSnapshot> {
    // Single pass: pick the newest session carrying token usage.
    let latest = cache
        .sessions
        .values()
        .filter(|s| s.has_token_usage)
        .max_by(|a, b| a.timestamp.cmp(&b.timestamp))?;

    Some(CodexSessionSnapshot {
        latest_input: latest.input_tokens,
        latest_output: latest.output_tokens,
        latest_cached: latest.cached_input_tokens,
        latest_context_window: latest.context_window,
        #[cfg(test)]
        latest_timestamp: Some(latest.timestamp.clone()),
    })
}

#[cfg(test)]
pub(crate) fn latest_codex_limits(cache: &CodexImportCache) -> Option<CodexRateLimits> {
    cache
        .latest_limits
        .clone()
        .or_else(|| find_latest_limits(&cache.sessions))
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

fn parse_codex_session_file(path: &Path, modified: SystemTime, file_len: u64) -> ParsedSessionFile {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return ParsedSessionFile::Unreadable,
    };
    let reader = BufReader::new(file);

    match parse_codex_session_reader(reader) {
        ParsedSessionContents::Parsed(data) => ParsedSessionFile::Parsed(CachedCodexSession {
            modified,
            file_len,
            timestamp: data.timestamp,
            input_tokens: data.input_tokens,
            output_tokens: data.output_tokens,
            cached_input_tokens: data.cached_input_tokens,
            context_window: data.context_window,
            has_token_usage: data.has_token_usage,
            limits: data.limits,
        }),
        ParsedSessionContents::NoUsageOrLimits => ParsedSessionFile::NoUsageOrLimits,
        ParsedSessionContents::ParseError => ParsedSessionFile::ParseError,
    }
}

#[cfg(test)]
fn parse_codex_session_contents(contents: &str) -> Option<CodexSessionData> {
    match parse_codex_session_contents_with_status(contents) {
        ParsedSessionContents::Parsed(parsed) => Some(parsed),
        ParsedSessionContents::NoUsageOrLimits | ParsedSessionContents::ParseError => None,
    }
}

#[cfg(test)]
fn parse_codex_session_contents_with_status(contents: &str) -> ParsedSessionContents {
    parse_codex_session_reader(std::io::Cursor::new(contents.as_bytes()))
}

fn parse_codex_session_reader<R: BufRead>(mut reader: R) -> ParsedSessionContents {
    let mut parsed_json_lines = 0_usize;
    let mut session_timestamp: Option<String> = None;
    let mut latest_event_timestamp: Option<String> = None;
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut cached_input_tokens: u64 = 0;
    let mut context_window: u64 = 0;
    let mut has_token_usage = false;
    let mut latest_limits: Option<CodexRateLimits> = None;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = match reader.read_line(&mut line) {
            Ok(count) => count,
            Err(_) => return ParsedSessionContents::ParseError,
        };
        if bytes_read == 0 {
            break;
        }

        let line = line.trim_end_matches(['\n', '\r']);
        if line.is_empty() {
            continue;
        }

        let Ok(parsed_line) = serde_json::from_str::<CodexSessionLine>(line) else {
            continue;
        };
        parsed_json_lines += 1;

        if parsed_line.event_type == "session_meta" {
            let meta_timestamp = parsed_line
                .payload
                .as_ref()
                .and_then(|payload| payload.timestamp.as_ref())
                .or(parsed_line.timestamp.as_ref());
            if let Some(ts) = meta_timestamp {
                session_timestamp = Some(ts.clone());
            }
            continue;
        }

        let is_token_count = parsed_line.event_type == "event_msg"
            && parsed_line
                .payload
                .as_ref()
                .and_then(|payload| payload.payload_type.as_deref())
                == Some("token_count");
        if !is_token_count {
            continue;
        }

        let event_timestamp = parsed_line.timestamp.as_ref().or(parsed_line
            .payload
            .as_ref()
            .and_then(|payload| payload.timestamp.as_ref()));
        if let Some(ts) = event_timestamp {
            latest_event_timestamp = Some(ts.clone());
        }

        let primary = parsed_line
            .payload
            .as_ref()
            .and_then(|payload| payload.rate_limits.as_ref())
            .and_then(|limits| limits.primary.as_ref())
            .map(parse_codex_rate_limit);
        let secondary = parsed_line
            .payload
            .as_ref()
            .and_then(|payload| payload.rate_limits.as_ref())
            .and_then(|limits| limits.secondary.as_ref())
            .map(parse_codex_rate_limit);
        if primary.is_some() || secondary.is_some() {
            let limit_timestamp = event_timestamp
                .cloned()
                .or_else(|| latest_event_timestamp.clone())
                .or_else(|| session_timestamp.clone())
                .unwrap_or_else(|| "unknown".to_string());
            latest_limits = Some(CodexRateLimits {
                timestamp: limit_timestamp,
                primary,
                secondary,
            });
        }

        let maybe_info = parsed_line
            .payload
            .as_ref()
            .and_then(|payload| payload.info.as_ref());

        if let Some(info) = maybe_info {
            if let Some(total_usage) = info.total_token_usage.as_ref() {
                input_tokens = total_usage.input_tokens;
                output_tokens = total_usage.output_tokens;
                cached_input_tokens = total_usage.cached_input_tokens;
                has_token_usage = true;
            }
            if let Some(window) = info.model_context_window {
                context_window = window;
            }
        }
    }

    if parsed_json_lines == 0 {
        return ParsedSessionContents::ParseError;
    }

    let timestamp = match latest_event_timestamp.or(session_timestamp) {
        Some(timestamp) => timestamp,
        None => return ParsedSessionContents::NoUsageOrLimits,
    };

    if !has_token_usage && latest_limits.is_none() {
        return ParsedSessionContents::NoUsageOrLimits;
    }

    ParsedSessionContents::Parsed(CodexSessionData {
        timestamp,
        input_tokens,
        output_tokens,
        cached_input_tokens,
        context_window,
        has_token_usage,
        limits: latest_limits,
    })
}

fn parse_codex_rate_limit(node: &CodexRawRateLimit) -> CodexRateLimit {
    CodexRateLimit {
        used_percent: node.used_percent.as_f64(),
        resets_at: node.resets_at,
    }
}

fn find_latest_limits(sessions: &HashMap<PathBuf, CachedCodexSession>) -> Option<CodexRateLimits> {
    sessions
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::models::AppConfig;

    #[test]
    fn parses_codex_session_usage_from_token_count_events() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:42.927Z","type":"session_meta","payload":{"timestamp":"2026-02-16T09:45:42.927Z"}}
{"timestamp":"2026-02-16T09:45:53.237Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":8582,"output_tokens":210}}}}
{"timestamp":"2026-02-16T09:45:56.220Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":17438,"output_tokens":326}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex usage");
        assert_eq!(parsed.timestamp, "2026-02-16T09:45:56.220Z");
        assert_eq!(parsed.input_tokens, 17438);
        assert_eq!(parsed.output_tokens, 326);
        assert!(parsed.has_token_usage);
        assert!(parsed.limits.is_none());
    }

    #[test]
    fn parses_codex_rate_limits() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:56.220Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":17438,"output_tokens":326}},"rate_limits":{"primary":{"used_percent":7.0,"window_minutes":300,"resets_at":1771243734},"secondary":{"used_percent":25.0,"window_minutes":10080,"resets_at":1771317088}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex usage");
        assert!(parsed.has_token_usage);
        let limits = parsed.limits.expect("expected limits");
    }

    #[test]
    fn parses_codex_rate_limits_with_integer_percent() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:56.220Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":20}},"rate_limits":{"primary":{"used_percent":7,"window_minutes":300,"resets_at":1771243734}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex usage");
        let limits = parsed.limits.expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 7.0);
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
        assert_eq!(parsed.timestamp, "2026-02-17T13:47:12.863Z");
        assert!(!parsed.has_token_usage);
        let limits = parsed.limits.expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 3.0);
        assert_eq!(limits.secondary.expect("secondary").used_percent, 2.0);
    }

    #[test]
    fn parses_rate_limits_without_event_timestamp_using_session_meta_timestamp() {
        let payload = r#"{"timestamp":"2026-02-17T13:47:00.000Z","type":"session_meta","payload":{"timestamp":"2026-02-17T13:47:00.000Z"}}
{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":6.0,"window_minutes":300,"resets_at":1771348283}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex limits");
        assert_eq!(parsed.timestamp, "2026-02-17T13:47:00.000Z");
        let limits = parsed.limits.expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 6.0);
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
                cached_input_tokens: 0,
                context_window: 0,
                has_token_usage: false,
                limits: Some(CodexRateLimits {
                    timestamp: "2026-02-18T00:00:00Z".to_string(),
                    primary: Some(CodexRateLimit {
                        used_percent: 12.0,
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
                cached_input_tokens: 0,
                context_window: 0,
                has_token_usage: false,
                limits: Some(CodexRateLimits {
                    timestamp: "2026-02-17T23:59:59Z".to_string(),
                    primary: Some(CodexRateLimit {
                        used_percent: 4.0,
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
        assert_eq!(parsed.timestamp, "2026-02-18T10:01:10.000Z");
        assert_eq!(parsed.input_tokens, 180);
        assert_eq!(parsed.output_tokens, 55);
        assert!(parsed.has_token_usage);
        let limits = parsed.limits.expect("expected limits");
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

        let mut cache = CodexImportCache::default();
        merge_codex_usage(&config, &mut cache);

        let snap = codex_session_snapshot(&cache).expect("expected snapshot");
        assert_eq!(snap.latest_input, 180);
        assert_eq!(snap.latest_output, 55);
        assert_eq!(
            snap.latest_timestamp.as_deref(),
            Some("2026-02-18T10:01:10.000Z")
        );

        let limits = latest_codex_limits(&cache).expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 9.0);
        assert_eq!(limits.secondary.expect("secondary").used_percent, 4.0);
        let diagnostics = &cache.diagnostics;
        assert_eq!(diagnostics.active_files, 3);
        assert_eq!(diagnostics.refreshed_files, 3);
        assert_eq!(diagnostics.parse_error_files, 0);
        assert_eq!(diagnostics.no_usage_or_limits_files, 1);
        assert_eq!(diagnostics.unreadable_files, 0);
        assert_eq!(diagnostics.discovery_interval, MIN_DISCOVERY_INTERVAL);
        assert!(diagnostics.last_import_at.is_some());

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    #[ignore = "performance probe for local profiling"]
    fn benchmark_collect_codex_session_files_large_tree() {
        let temp_root = make_temp_dir("codex-scan-bench");
        for day in 1..=10 {
            let day_dir = temp_root.join("2026").join("02").join(format!("{day:02}"));
            fs::create_dir_all(&day_dir).expect("create day dir");
            for file_idx in 0..250 {
                let file_path = day_dir.join(format!("rollout-{file_idx:04}.jsonl"));
                fs::write(
                    file_path,
                    "{\"timestamp\":\"2026-02-18T10:00:00.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1,\"output_tokens\":1}}}}\n",
                )
                .expect("write benchmark fixture");
            }
        }

        let started = Instant::now();
        let files = collect_codex_session_files(&temp_root).expect("expected files");
        let elapsed = started.elapsed();
        assert_eq!(files.len(), 2500);
        eprintln!(
            "collect_codex_session_files scanned {} files in {:?}",
            files.len(),
            elapsed
        );

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn discovery_backoff_increases_when_idle_and_resets_on_change() {
        let temp_root = make_temp_dir("codex-backoff");
        let mut config = AppConfig::default();
        config.codex_import.enabled = true;
        config.codex_import.sessions_dir = Some(temp_root.to_string_lossy().to_string());
        let mut cache = CodexImportCache::default();

        assert_eq!(cache.session_discovery_interval, MIN_DISCOVERY_INTERVAL);

        for _ in 0..3 {
            cache.last_discovery_at = Some(SystemTime::now() - Duration::from_secs(3600));
            merge_codex_usage(&config, &mut cache);
        }
        assert_eq!(
            cache.session_discovery_interval,
            MIN_DISCOVERY_INTERVAL + DISCOVERY_BACKOFF_STEP
        );

        let session_dir = temp_root.join("2026").join("02").join("18");
        fs::create_dir_all(&session_dir).expect("create session dir");
        write_fixture(&session_dir, "mixed_usage_and_limits.jsonl");

        cache.last_discovery_at = Some(SystemTime::now() - Duration::from_secs(3600));
        merge_codex_usage(&config, &mut cache);
        assert_eq!(cache.session_discovery_interval, MIN_DISCOVERY_INTERVAL);

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn parser_classifies_malformed_only_payload_as_parse_error() {
        let payload = "not-json\nthis is also invalid\n";
        let classification = parse_codex_session_contents_with_status(payload);
        assert!(matches!(classification, ParsedSessionContents::ParseError));
    }

    #[test]
    fn parser_classifies_valid_non_usage_payload_as_no_usage_or_limits() {
        let payload = "{\"timestamp\":\"2026-02-16T09:45:42.927Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\"}}";
        let classification = parse_codex_session_contents_with_status(payload);
        assert!(matches!(
            classification,
            ParsedSessionContents::NoUsageOrLimits
        ));
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

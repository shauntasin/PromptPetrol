use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;

use crate::models::AppConfig;

const MIN_DISCOVERY_INTERVAL: Duration = Duration::from_secs(10);
const MAX_DISCOVERY_INTERVAL: Duration = Duration::from_secs(120);
const DISCOVERY_BACKOFF_STEP: Duration = Duration::from_secs(10);
const PREFIX_FINGERPRINT_BYTES: usize = 256;

#[derive(Debug, Clone)]
struct CachedCodexSession {
    modified: SystemTime,
    file_len: u64,
    parsed_len: u64,
    file_identity: Option<FileIdentity>,
    prefix_fingerprint: u64,
    boundary_fingerprint: u64,
    session_timestamp: Option<String>,
    latest_event_timestamp: Option<String>,
    timestamp: String,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    context_window: u64,
    has_token_usage: bool,
    limits: Option<CodexRateLimits>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexImportDiagnostics {
    pub(crate) active_files: usize,
    pub(crate) refreshed_files: usize,
    pub(crate) parse_error_files: usize,
    pub(crate) no_usage_or_limits_files: usize,
    pub(crate) unreadable_files: usize,
    pub(crate) last_attempt_at: Option<SystemTime>,
    pub(crate) last_success_at: Option<SystemTime>,
    pub(crate) last_duration: Option<Duration>,
    pub(crate) consecutive_failures: u32,
    pub(crate) discovery_interval: Duration,
    pub(crate) discovery_error: Option<String>,
}

impl Default for CodexImportDiagnostics {
    fn default() -> Self {
        Self {
            active_files: 0,
            refreshed_files: 0,
            parse_error_files: 0,
            no_usage_or_limits_files: 0,
            unreadable_files: 0,
            last_attempt_at: None,
            last_success_at: None,
            last_duration: None,
            consecutive_failures: 0,
            discovery_interval: MIN_DISCOVERY_INTERVAL,
            discovery_error: None,
        }
    }
}

enum ParsedSessionFile {
    Parsed(Box<CachedCodexSession>),
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
    parsed_len: u64,
    session_timestamp: Option<String>,
    latest_event_timestamp: Option<String>,
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
    used_percent: f64,
    #[serde(default)]
    resets_at: Option<u64>,
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

#[derive(Debug, Clone)]
pub(crate) struct CodexImportCache {
    sessions: HashMap<PathBuf, CachedCodexSession>,
    pub(crate) latest_limits: Option<CodexRateLimits>,
    latest_session: Option<CodexSessionSnapshot>,
    session_files: Vec<PathBuf>,
    last_discovery_at: Option<Instant>,
    session_discovery_interval: Duration,
    idle_discovery_cycles: u32,
    source_dir: Option<PathBuf>,
    pub(crate) diagnostics: CodexImportDiagnostics,
}

impl Default for CodexImportCache {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
            latest_limits: None,
            latest_session: None,
            session_files: Vec::new(),
            last_discovery_at: None,
            session_discovery_interval: MIN_DISCOVERY_INTERVAL,
            idle_discovery_cycles: 0,
            source_dir: None,
            diagnostics: CodexImportDiagnostics::default(),
        }
    }
}

impl CodexImportCache {
    pub(crate) fn force_discovery(&mut self) {
        self.last_discovery_at = None;
    }

    pub(crate) fn ui_snapshot(&self) -> Self {
        Self {
            latest_limits: self.latest_limits.clone(),
            latest_session: self.latest_session.clone(),
            diagnostics: self.diagnostics.clone(),
            ..Self::default()
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
                parsed_len: 0,
                file_identity: None,
                prefix_fingerprint: 0,
                boundary_fingerprint: 0,
                session_timestamp: Some("2026-06-18T00:00:00Z".to_string()),
                latest_event_timestamp: None,
                timestamp: "2026-06-18T00:00:00Z".to_string(),
                input_tokens: input,
                output_tokens: output,
                cached_input_tokens: cached,
                context_window: window,
                has_token_usage: true,
                limits: None,
            },
        );
        cache.latest_session = Some(CodexSessionSnapshot {
            latest_input: input,
            latest_output: output,
            latest_cached: cached,
            latest_context_window: window,
            latest_timestamp: Some("2026-06-18T00:00:00Z".to_string()),
        });
        cache
    }
}

pub(crate) fn merge_codex_usage(config: &AppConfig, cache: &mut CodexImportCache) {
    if !config.codex_import.enabled {
        *cache = CodexImportCache::default();
        return;
    }

    let started = Instant::now();
    let attempt_at = SystemTime::now();
    let sessions_dir = codex_sessions_dir(config);
    if cache.source_dir.as_ref() != Some(&sessions_dir) {
        *cache = CodexImportCache::default();
        cache.source_dir = Some(sessions_dir.clone());
    }
    let previous_success_at = cache.diagnostics.last_success_at;
    let previous_failures = cache.diagnostics.consecutive_failures;
    let mut changes_detected = false;
    let mut discovery_ran = false;
    let mut discovery_error = cache.diagnostics.discovery_error.clone();
    if should_refresh_file_discovery(cache) {
        discovery_ran = true;
        match collect_codex_session_files(&sessions_dir) {
            Ok(files) => {
                changes_detected |= files != cache.session_files;
                cache.session_files = files;
                discovery_error = None;
            }
            Err(error) => discovery_error = Some(error.to_string()),
        }
        cache.last_discovery_at = Some(Instant::now());
    }

    // `session_files` is the authoritative active set; only refresh entries whose
    // mtime/len changed, and drop cached sessions whose file is gone or invalid.
    let mut refreshed_files = 0_usize;
    let mut parse_error_files = 0_usize;
    let mut no_usage_or_limits_files = 0_usize;
    let mut unreadable_files = 0_usize;
    let files = std::mem::take(&mut cache.session_files);
    for file in &files {
        let metadata = match fs::metadata(file) {
            Ok(metadata) => metadata,
            Err(_) => {
                changes_detected = true;
                unreadable_files += 1;
                cache.sessions.remove(file);
                continue;
            }
        };
        let modified = match metadata.modified() {
            Ok(modified) => modified,
            Err(_) => {
                changes_detected = true;
                unreadable_files += 1;
                cache.sessions.remove(file);
                continue;
            }
        };
        let file_len = metadata.len();
        let file_identity = file_identity(&metadata);

        let needs_refresh = cache.sessions.get(file).is_none_or(|cached| {
            cached.modified != modified
                || cached.file_len != file_len
                || cached.file_identity != file_identity
        });
        if !needs_refresh {
            continue;
        }
        changes_detected = true;
        refreshed_files += 1;

        match parse_codex_session_file(
            file,
            modified,
            file_len,
            file_identity,
            cache.sessions.get(file),
        ) {
            ParsedSessionFile::Parsed(parsed) => {
                cache.sessions.insert(file.clone(), *parsed);
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
    if discovery_ran {
        let active_paths: HashSet<&PathBuf> = files.iter().collect();
        cache.sessions.retain(|path, _| active_paths.contains(path));
    }
    let active_count = files.len();
    cache.session_files = files;
    if changes_detected {
        cache.latest_limits = find_latest_limits(&cache.sessions);
        cache.latest_session = find_latest_session(&cache.sessions);
    }
    if changes_detected || discovery_ran {
        tune_discovery_interval(cache, changes_detected);
    }
    cache.diagnostics = CodexImportDiagnostics {
        active_files: active_count,
        refreshed_files,
        parse_error_files,
        no_usage_or_limits_files,
        unreadable_files,
        last_attempt_at: Some(attempt_at),
        last_success_at: if discovery_error.is_none() {
            Some(attempt_at)
        } else {
            previous_success_at
        },
        last_duration: Some(started.elapsed()),
        consecutive_failures: if discovery_error.is_none() {
            0
        } else {
            previous_failures.saturating_add(1)
        },
        discovery_interval: cache.session_discovery_interval,
        discovery_error,
    };
}

fn should_refresh_file_discovery(cache: &CodexImportCache) -> bool {
    let Some(last_discovery) = cache.last_discovery_at else {
        return true;
    };
    last_discovery.elapsed() >= cache.session_discovery_interval
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
#[derive(Debug, Clone)]
pub(crate) struct CodexSessionSnapshot {
    pub(crate) latest_input: u64,
    pub(crate) latest_output: u64,
    pub(crate) latest_cached: u64,
    pub(crate) latest_context_window: u64,
    #[cfg(test)]
    pub(crate) latest_timestamp: Option<String>,
}

pub(crate) fn codex_session_snapshot(cache: &CodexImportCache) -> Option<CodexSessionSnapshot> {
    cache.latest_session.clone()
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

fn collect_codex_session_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut directories = vec![dir.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if directory == dir => return Err(error),
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            // Do not follow symlinks: a linked ancestor can form an infinite cycle.
            if file_type.is_dir() {
                directories.push(path);
            } else if file_type.is_file()
                && path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
            {
                files.push(path);
            }
        }
    }
    files.sort_unstable();
    Ok(files)
}

fn file_identity(metadata: &fs::Metadata) -> Option<FileIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        Some(FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

fn parse_codex_session_file(
    path: &Path,
    modified: SystemTime,
    file_len: u64,
    file_identity: Option<FileIdentity>,
    cached: Option<&CachedCodexSession>,
) -> ParsedSessionFile {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return ParsedSessionFile::Unreadable,
    };
    let prefix_fingerprint = file_prefix_fingerprint(path).unwrap_or_default();

    let append = cached.is_some_and(|cached| {
        file_len > cached.file_len
            && cached.parsed_len <= file_len
            && cached.file_identity == file_identity
            && cached.prefix_fingerprint == prefix_fingerprint
            && cached.boundary_fingerprint
                == file_boundary_fingerprint(path, cached.parsed_len).unwrap_or_default()
    });
    let mut file = file;
    let mut parser = cached
        .filter(|_| append)
        .map(CodexSessionParser::from_cached)
        .unwrap_or_default();
    let start_offset = if append {
        cached.map_or(0, |cached| cached.parsed_len)
    } else {
        0
    };
    if start_offset > 0 && file.seek(SeekFrom::Start(start_offset)).is_err() {
        return ParsedSessionFile::Unreadable;
    }

    match parser.parse_reader(BufReader::new(file), start_offset, false) {
        Ok(_) => match parser.finish() {
            ParsedSessionContents::Parsed(data) => {
                let parsed_len = data.parsed_len;
                let boundary_fingerprint =
                    file_boundary_fingerprint(path, parsed_len).unwrap_or_default();
                ParsedSessionFile::Parsed(Box::new(CachedCodexSession {
                    modified,
                    file_len,
                    parsed_len,
                    file_identity,
                    prefix_fingerprint,
                    boundary_fingerprint,
                    session_timestamp: data.session_timestamp,
                    latest_event_timestamp: data.latest_event_timestamp,
                    timestamp: data.timestamp,
                    input_tokens: data.input_tokens,
                    output_tokens: data.output_tokens,
                    cached_input_tokens: data.cached_input_tokens,
                    context_window: data.context_window,
                    has_token_usage: data.has_token_usage,
                    limits: data.limits,
                }))
            }
            ParsedSessionContents::NoUsageOrLimits => ParsedSessionFile::NoUsageOrLimits,
            ParsedSessionContents::ParseError => ParsedSessionFile::ParseError,
        },
        Err(_) => ParsedSessionFile::Unreadable,
    }
}

fn file_prefix_fingerprint(path: &Path) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let mut prefix = [0_u8; PREFIX_FINGERPRINT_BYTES];
    let bytes_read = file.read(&mut prefix)?;
    Ok(fingerprint(&prefix[..bytes_read]))
}

fn file_boundary_fingerprint(path: &Path, committed_len: u64) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let start = committed_len.saturating_sub(PREFIX_FINGERPRINT_BYTES as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut boundary = [0_u8; PREFIX_FINGERPRINT_BYTES];
    let bytes_read = file.read(&mut boundary)?;
    Ok(fingerprint(&boundary[..bytes_read]))
}

fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211_u64);
    }
    hash ^ bytes.len() as u64
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

#[derive(Default)]
struct CodexSessionParser {
    parsed_json_lines: usize,
    parsed_len: u64,
    session_timestamp: Option<String>,
    latest_event_timestamp: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    context_window: u64,
    has_token_usage: bool,
    latest_limits: Option<CodexRateLimits>,
}

impl CodexSessionParser {
    fn from_cached(cached: &CachedCodexSession) -> Self {
        Self {
            parsed_json_lines: 1,
            parsed_len: cached.parsed_len,
            session_timestamp: cached.session_timestamp.clone(),
            latest_event_timestamp: cached.latest_event_timestamp.clone(),
            input_tokens: cached.input_tokens,
            output_tokens: cached.output_tokens,
            cached_input_tokens: cached.cached_input_tokens,
            context_window: cached.context_window,
            has_token_usage: cached.has_token_usage,
            latest_limits: cached.limits.clone(),
        }
    }

    fn parse_reader<R: BufRead>(
        &mut self,
        mut reader: R,
        start_offset: u64,
        allow_partial_line: bool,
    ) -> io::Result<u64> {
        let mut committed_len = start_offset;
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line)?;
            if bytes_read == 0 {
                break;
            }

            let complete = line.ends_with('\n');
            if complete || allow_partial_line {
                committed_len += bytes_read as u64;
                self.accept_line(line.trim_end_matches(['\n', '\r']));
            }
            if !complete {
                break;
            }
        }

        self.parsed_len = committed_len;
        Ok(committed_len)
    }

    fn accept_line(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }

        let Ok(parsed_line) = serde_json::from_str::<CodexSessionLine>(line) else {
            return;
        };
        self.parsed_json_lines += 1;

        if parsed_line.event_type == "session_meta" {
            let meta_timestamp = parsed_line
                .payload
                .as_ref()
                .and_then(|payload| payload.timestamp.as_ref())
                .or(parsed_line.timestamp.as_ref());
            if let Some(ts) = meta_timestamp {
                self.session_timestamp = Some(ts.clone());
            }
            return;
        }

        let is_token_count = parsed_line.event_type == "event_msg"
            && parsed_line
                .payload
                .as_ref()
                .and_then(|payload| payload.payload_type.as_deref())
                == Some("token_count");
        if !is_token_count {
            return;
        }

        let event_timestamp = parsed_line.timestamp.as_ref().or(parsed_line
            .payload
            .as_ref()
            .and_then(|payload| payload.timestamp.as_ref()));
        if let Some(ts) = event_timestamp {
            self.latest_event_timestamp = Some(ts.clone());
        }

        let primary = parsed_line
            .payload
            .as_ref()
            .and_then(|payload| payload.rate_limits.as_ref())
            .and_then(|limits| limits.primary.as_ref())
            .and_then(parse_codex_rate_limit);
        let secondary = parsed_line
            .payload
            .as_ref()
            .and_then(|payload| payload.rate_limits.as_ref())
            .and_then(|limits| limits.secondary.as_ref())
            .and_then(parse_codex_rate_limit);
        if primary.is_some() || secondary.is_some() {
            let limit_timestamp = event_timestamp
                .cloned()
                .or_else(|| self.latest_event_timestamp.clone())
                .or_else(|| self.session_timestamp.clone())
                .unwrap_or_else(|| "unknown".to_string());
            self.latest_limits = Some(CodexRateLimits {
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
                self.input_tokens = total_usage.input_tokens;
                self.output_tokens = total_usage.output_tokens;
                self.cached_input_tokens = total_usage.cached_input_tokens;
                self.has_token_usage = true;
            }
            if let Some(window) = info.model_context_window {
                self.context_window = window;
            }
        }
    }

    fn finish(self) -> ParsedSessionContents {
        if self.parsed_json_lines == 0 {
            return ParsedSessionContents::ParseError;
        }

        let timestamp = match self
            .latest_event_timestamp
            .clone()
            .or_else(|| self.session_timestamp.clone())
        {
            Some(timestamp) => timestamp,
            None => return ParsedSessionContents::NoUsageOrLimits,
        };

        if !self.has_token_usage && self.latest_limits.is_none() {
            return ParsedSessionContents::NoUsageOrLimits;
        }

        ParsedSessionContents::Parsed(CodexSessionData {
            parsed_len: self.parsed_len,
            session_timestamp: self.session_timestamp,
            latest_event_timestamp: self.latest_event_timestamp,
            timestamp,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cached_input_tokens: self.cached_input_tokens,
            context_window: self.context_window,
            has_token_usage: self.has_token_usage,
            limits: self.latest_limits,
        })
    }
}

#[cfg(test)]
fn parse_codex_session_reader<R: BufRead>(reader: R) -> ParsedSessionContents {
    let mut parser = CodexSessionParser::default();
    if parser.parse_reader(reader, 0, true).is_err() {
        return ParsedSessionContents::ParseError;
    }
    parser.finish()
}

fn parse_codex_rate_limit(node: &CodexRawRateLimit) -> Option<CodexRateLimit> {
    if !node.used_percent.is_finite() || node.used_percent < 0.0 {
        return None;
    }
    Some(CodexRateLimit {
        used_percent: node.used_percent,
        resets_at: node.resets_at,
    })
}

fn find_latest_session(
    sessions: &HashMap<PathBuf, CachedCodexSession>,
) -> Option<CodexSessionSnapshot> {
    let latest = sessions
        .values()
        .filter(|session| session.has_token_usage)
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
    use std::io::Write;
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
        assert_eq!(limits.primary.expect("primary").used_percent, 7.0);
        assert_eq!(limits.secondary.expect("secondary").used_percent, 25.0);
    }

    #[test]
    fn parses_codex_rate_limits_with_integer_percent() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:56.220Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":20}},"rate_limits":{"primary":{"used_percent":7,"window_minutes":300,"resets_at":1771243734}}}}"#;
        let parsed = parse_codex_session_contents(payload).expect("expected codex usage");
        let limits = parsed.limits.expect("expected limits");
        assert_eq!(limits.primary.expect("primary").used_percent, 7.0);
    }

    #[test]
    fn ignores_negative_rate_limits() {
        let payload = r#"{"timestamp":"2026-02-16T09:45:56.220Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":-1.0}}}}"#;
        let parsed = parse_codex_session_contents_with_status(payload);

        assert!(matches!(parsed, ParsedSessionContents::NoUsageOrLimits));
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
                parsed_len: 100,
                file_identity: None,
                prefix_fingerprint: 0,
                boundary_fingerprint: 0,
                session_timestamp: None,
                latest_event_timestamp: Some("2026-02-18T00:00:00Z".to_string()),
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
                parsed_len: 110,
                file_identity: None,
                prefix_fingerprint: 0,
                boundary_fingerprint: 0,
                session_timestamp: None,
                latest_event_timestamp: Some("2026-02-17T23:59:59Z".to_string()),
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
        assert!(diagnostics.last_attempt_at.is_some());

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
            cache.force_discovery();
            merge_codex_usage(&config, &mut cache);
        }
        assert_eq!(
            cache.session_discovery_interval,
            MIN_DISCOVERY_INTERVAL + DISCOVERY_BACKOFF_STEP
        );

        let session_dir = temp_root.join("2026").join("02").join("18");
        fs::create_dir_all(&session_dir).expect("create session dir");
        write_fixture(&session_dir, "mixed_usage_and_limits.jsonl");

        cache.force_discovery();
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

    #[test]
    fn disabling_import_clears_cached_sessions_and_limits() {
        let mut cache = CodexImportCache::with_test_context(100, 20, 10, 200);
        cache.latest_limits = Some(CodexRateLimits {
            timestamp: "2026-08-19T00:00:00Z".into(),
            primary: Some(CodexRateLimit {
                used_percent: 50.0,
                resets_at: None,
            }),
            secondary: None,
        });
        let mut config = AppConfig::default();
        config.codex_import.enabled = false;

        merge_codex_usage(&config, &mut cache);

        assert!(codex_session_snapshot(&cache).is_none());
        assert!(cache.latest_limits.is_none());
        assert!(cache.session_files.is_empty());
        assert!(cache.source_dir.is_none());
    }

    #[test]
    fn changing_sessions_directory_invalidates_cache_immediately() {
        let root = make_temp_dir("source-switch");
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).expect("create first source");
        fs::create_dir_all(&second).expect("create second source");
        write_token_session(&first.join("session.jsonl"), 100);
        write_token_session(&second.join("session.jsonl"), 900);

        let mut config = AppConfig::default();
        config.codex_import.sessions_dir = Some(first.to_string_lossy().into_owned());
        let mut cache = CodexImportCache::default();
        merge_codex_usage(&config, &mut cache);
        assert_eq!(
            codex_session_snapshot(&cache)
                .expect("first source snapshot")
                .latest_input,
            100
        );

        config.codex_import.sessions_dir = Some(second.to_string_lossy().into_owned());
        merge_codex_usage(&config, &mut cache);
        assert_eq!(
            codex_session_snapshot(&cache)
                .expect("second source snapshot")
                .latest_input,
            900
        );

        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn append_refresh_reuses_state_and_waits_for_a_complete_jsonl_record() {
        let root = make_temp_dir("codex-incremental");
        let session = root.join("session.jsonl");
        write_token_session(&session, 100);

        let mut config = AppConfig::default();
        config.codex_import.sessions_dir = Some(root.to_string_lossy().into_owned());
        let mut cache = CodexImportCache::default();
        merge_codex_usage(&config, &mut cache);

        assert_eq!(
            codex_session_snapshot(&cache)
                .expect("initial snapshot")
                .latest_input,
            100
        );
        let initial_parsed_len = cache
            .sessions
            .get(&session)
            .expect("cached session")
            .parsed_len;

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&session)
            .expect("open session for append");
        let appended = token_session_line(200);
        file.write_all(appended.as_bytes())
            .expect("write partial event");
        merge_codex_usage(&config, &mut cache);

        let cached = cache
            .sessions
            .get(&session)
            .expect("cached partial session");
        assert_eq!(cached.parsed_len, initial_parsed_len);
        assert_eq!(
            codex_session_snapshot(&cache)
                .expect("snapshot while partial")
                .latest_input,
            100
        );

        file.write_all(b"\n").expect("complete event");
        merge_codex_usage(&config, &mut cache);
        assert_eq!(
            codex_session_snapshot(&cache)
                .expect("snapshot after append")
                .latest_input,
            200
        );
        assert!(
            cache
                .sessions
                .get(&session)
                .expect("completed session")
                .parsed_len
                > initial_parsed_len
        );

        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn replacement_with_a_longer_prefix_rebuilds_instead_of_replaying_old_state() {
        let root = make_temp_dir("codex-replacement");
        let session = root.join("session.jsonl");
        write_token_session(&session, 100);

        let mut config = AppConfig::default();
        config.codex_import.sessions_dir = Some(root.to_string_lossy().into_owned());
        let mut cache = CodexImportCache::default();
        merge_codex_usage(&config, &mut cache);
        assert!(codex_session_snapshot(&cache).is_some());

        let replacement = concat!(
            "{\"timestamp\":\"2026-08-20T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"timestamp\":\"2026-08-20T00:00:00Z\"}}\n",
            "{\"timestamp\":\"2026-08-20T00:00:01Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\"}}\n",
            "{\"timestamp\":\"2026-08-20T00:00:02Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\"}}\n"
        );
        assert!(replacement.len() as u64 > session.metadata().expect("metadata").len());
        fs::write(&session, replacement).expect("replace session");

        merge_codex_usage(&config, &mut cache);

        assert!(codex_session_snapshot(&cache).is_none());
        assert!(!cache.sessions.contains_key(&session));
        assert_eq!(cache.diagnostics.no_usage_or_limits_files, 1);

        fs::remove_dir_all(root).expect("remove test directory");
    }

    fn fixture_contents(name: &str) -> String {
        fs::read_to_string(fixture_path(name)).expect("read fixture file")
    }

    fn write_fixture(target_dir: &Path, fixture_name: &str) {
        let contents = fixture_contents(fixture_name);
        let target = target_dir.join(fixture_name);
        fs::write(target, contents).expect("write fixture");
    }

    fn write_token_session(path: &Path, input_tokens: u64) {
        let contents = format!("{}\n", token_session_line(input_tokens));
        fs::write(path, contents).expect("write token session");
    }

    fn token_session_line(input_tokens: u64) -> String {
        format!(
            r#"{{"timestamp":"2026-08-19T00:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input_tokens},"output_tokens":10}},"model_context_window":1000}}}}}}"#
        )
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

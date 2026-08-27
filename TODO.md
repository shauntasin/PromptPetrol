# TODO

This is the execution checklist for the research-backed
[scaling plan](docs/SCALING_PLAN.md). Complete phases in order. A phase is not
complete until its exit gate passes.

## S0 - Baseline And Contracts

- [ ] S0.1 Define fixture profiles for 2,500 files, 10,000 files, 100 MiB cold ingest, and a 64 KiB append
- [ ] S0.2 Replace the ignored wall-clock probe with Criterion benchmarks for discovery, cold ingest, warm refresh, append ingest, cache handoff, and rendering
- [ ] S0.3 Record CPU time, wall time, peak memory, bytes read, and metadata calls on the reference macOS machine
- [ ] S0.4 Add non-flaky CI smoke limits; keep statistical regression comparisons on controlled hardware
- [ ] S0.5 Document the normalized `ProviderSnapshot`, `UsageWindow`, `ContextWindow`, and `SourceHealth` contracts
- [ ] S0.6 Add fixture cases for a partial final JSONL line, truncation, replacement, clock regression, malformed append, and reset transition

**Baseline:** the current release probe scanned 2,500 one-line files in 6.67 ms
on 2026-08-19. It measures recursive discovery only, not metadata traversal,
parsing, cache cloning, rendering, or 10,000-file behavior.

**Exit gate:** benchmark commands and fixture profiles are reproducible, and
the provisional budgets in `docs/SCALING_PLAN.md` are either accepted or
revised from measured data.

## S1 - Source Trust

- [ ] S1.1 Track last attempt, last success, source timestamp, snapshot age, refresh duration, consecutive failures, and next retry per source
- [ ] S1.2 Add `LIVE`, `STALE`, `AUTH`, `RATE LIMITED`, `OFFLINE`, and `DISABLED` source states
- [ ] S1.3 Keep last-known-good values visible on transient failures and mark them stale
- [ ] S1.4 Show detailed, redacted source/config errors in the checklist view
- [ ] S1.5 Inject the Claude HTTP transport and test 200, 401, 403, 429, 5xx, malformed JSON, connect timeout, and response timeout
- [ ] S1.6 Parse both forms of `Retry-After` and add capped exponential backoff with deterministic jitter tests
- [ ] S1.7 Use `Instant` for in-process deadlines and durations; reserve `SystemTime` for display and persisted timestamps
- [ ] S1.8 Simplify terminal setup around Ratatui 0.30 initialization and verify cleanup on normal exit, error, and panic
- [ ] S1.9 Validate live Claude and Codex data without storing or printing credentials

**Exit gate:** every displayed value communicates freshness, all HTTP/error
classes have deterministic tests, and the terminal is restored through every
tested exit path.

## S2 - Snapshot And Runtime Boundary

- [ ] S2.1 Add provider-neutral domain types in a module that imports neither Codex nor Claude implementation types
- [ ] S2.2 Make the UI consume a small immutable `UiSnapshot`, not importer caches
- [ ] S2.3 Precompute latest context and rate-limit snapshots during ingest so rendering is O(1) in session count
- [ ] S2.4 Replace per-refresh thread creation with persistent source workers and bounded command/result channels
- [ ] S2.5 Keep the mutable Codex cache inside its worker; remove full-cache cloning from the UI thread
- [ ] S2.6 Render immediately at startup in `ACQUIRING` state instead of performing synchronous network/filesystem work
- [ ] S2.7 Reuse one configured Reqwest client inside the Claude worker
- [ ] S2.8 Cache Keychain resolution and invalidate it on auth failure, config change, or manual credential refresh
- [ ] S2.9 Give Codex, Claude, config reload, and full reconciliation independent deadlines
- [ ] S2.10 Coalesce repeated manual refresh commands without dropping a required follow-up refresh

**Exit gate:** no UI-thread operation is proportional to session count, startup
paints before network completion, and source workers cannot create unbounded
queues or overlapping requests.

## S3 - Incremental Codex Ingest

- [ ] S3.1 Split parser state from file I/O so the same state machine handles full and appended input
- [ ] S3.2 Store last committed byte offset and retain an incomplete trailing line without advancing the cursor
- [ ] S3.3 Parse only bytes appended after the committed offset
- [ ] S3.4 Detect truncation, replacement, incompatible file identity, and source-directory changes and rebuild affected state
- [ ] S3.5 Add stable deduplication for replayed events and idempotence tests across repeated refreshes
- [ ] S3.6 Introduce `notify` as a change accelerator with event coalescing
- [ ] S3.7 Trigger full reconciliation when `notify::Event::need_rescan()` is true
- [ ] S3.8 Retain periodic recursive reconciliation and polling fallback for missed events, network filesystems, and watcher failure
- [ ] S3.9 Stop issuing metadata calls for every historical file on every normal refresh
- [ ] S3.10 Persist a versioned ingest index only after cursor recovery tests pass; write it transactionally

**Exit gate:** append work scales with appended bytes, watcher loss self-heals,
and restart/truncation/replacement tests produce the same snapshot as a clean
full parse.

## S4 - History And Forecasting

- [ ] S4.1 Approve history semantics: sample-on-change, heartbeat interval, retention, reset boundaries, and clock policy
- [ ] S4.2 Add a versioned SQLite schema with migrations and uniqueness constraints
- [ ] S4.3 Store normalized usage samples and source health only; never store prompts or raw session JSONL
- [ ] S4.4 Measure default journal mode versus WAL before enabling WAL and define checkpoint behavior if selected
- [ ] S4.5 Add retention compaction, database-size diagnostics, backup/export, and corruption recovery tests
- [ ] S4.6 Calculate burn rate only from comparable samples inside the same provider window
- [ ] S4.7 Add confidence-qualified forecasts for threshold crossing and projected utilization at reset
- [ ] S4.8 Add trend/runway MFD page with honest insufficient-data and reset states

**Exit gate:** history survives restart and migration, duplicate samples are
rejected, retention is bounded, and forecasts are suppressed when the sample
quality is insufficient.

## S5 - Alerts And MFD Pages

- [ ] S5.1 Add Summary, Trends, and Data Link pages with keyboard navigation and responsive full/medium/compact layouts
- [ ] S5.2 Add configurable caution/critical thresholds with hysteresis
- [ ] S5.3 Add alert cooldown and transition-based delivery to prevent repeated notifications
- [ ] S5.4 Add alerts for stale data, authentication failure, unexpected reset, and counter regression
- [ ] S5.5 Add golden buffer tests for every page at 120x40, 120x20, 60x20, 40x12, and minimum size
- [ ] S5.6 Verify color and information hierarchy in true color, 256 color, and `NO_COLOR` modes

**Exit gate:** alerts fire once per state transition, every page remains usable
at supported breakpoints, and snapshots cover all data/source states.

## S6 - Provider And Distribution Expansion

- [ ] S6.1 Add a provider adapter trait only when implementing a third real provider
- [ ] S6.2 Keep billing/cost records separate from subscription-window utilization if spend tracking is approved
- [ ] S6.3 Add platform-specific secure credential stores before claiming Linux or Windows credential support
- [ ] S6.4 Produce reproducible release binaries, checksums, changelog, and Homebrew formula
- [ ] S6.5 Add macOS signing/notarization and Linux terminal compatibility validation
- [ ] S6.6 Consider daemon/remote-agent mode only after local worker, history, and security gates pass

**Exit gate:** each supported platform has tested installation, credential,
terminal, upgrade, and uninstall paths.

## Release Gates

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test --all-targets`
- [x] Run ignored visual tests and the 2,500-file discovery probe
- [x] Run `cargo audit` against the locked dependency graph
- [ ] Complete S0 benchmark suite, including the real 10,000-file profile
- [ ] Validate live Claude and Codex data on a real terminal
- [ ] Capture full, medium, compact, and minimum-size release screenshots

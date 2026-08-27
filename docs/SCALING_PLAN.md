# Scaling Plan

**Status:** researched implementation plan
**Baseline date:** 2026-08-19
**Scope:** local PromptPetrol TUI, Claude usage API, and Codex JSONL sessions

## Executive Decision

Keep PromptPetrol local-first through the history and forecasting milestones.
The next architecture should remove work proportional to historical session
count from the UI thread, make source freshness explicit, and make Codex ingest
incremental. It should not introduce Tokio, a hosted backend, a plugin ABI, or a
remote daemon yet.

The renderer is already decoupled from refresh work and is not the primary
scaling risk. Current scaling costs come from:

1. Searching all cached sessions for the latest context during every render.
2. Cloning the entire Codex cache on the UI thread for each refresh.
3. Calling metadata on every known session during normal refreshes.
4. Reparsing a growing JSONL file from byte zero.
5. Coupling source schedules and rebuilding HTTP/credential resources.
6. Displaying last-known values without a complete freshness contract.

## Verified Baseline

- All 33 tests, including ignored visual/probe tests, passed before this plan was written.
- The release-mode discovery probe scanned 2,500 one-line files in 6.67 ms on the current macOS host.
- That probe measures directory traversal only. It does not represent 10,000 files, realistic byte volume, metadata cost, cold parsing, cache cloning, incremental parsing, or rendering.
- Full MFD rendering is verified at 120x40 and the medium MFD at 120x20.

## Research Conclusions

### Timing

Use `Instant` for process-local scheduling, retry deadlines, and duration
measurement. Rust documents `Instant` as monotonically nondecreasing, while
`SystemTime` represents wall-clock time and can move due to clock correction.
Keep `SystemTime` or UTC timestamps only for persisted observations and display.

Source: [Rust `Instant` documentation](https://doc.rust-lang.org/std/time/struct.Instant.html)

### HTTP Client And Retry Policy

Keep one Reqwest client in the Claude worker. Reqwest documents that `Client`
owns a connection pool and should be reused. Treat 401/403 as credential states,
429 as rate limiting, and retry eligible transient failures with capped
exponential backoff. Honor `Retry-After`, whose value can be an HTTP date or a
delay in seconds.

Sources: [Reqwest `Client`](https://docs.rs/reqwest/0.12.28/reqwest/blocking/struct.Client.html),
[RFC 9110 Retry-After](https://www.rfc-editor.org/rfc/rfc9110.html#section-10.2.3),
[RFC 6585 429](https://www.rfc-editor.org/rfc/rfc6585.html#section-4)

### Filesystem Notifications

Use `notify` only as an accelerator. Its documentation calls out missed events
for very large trees, network-filesystem limitations, platform watch limits,
and `Event::need_rescan()`. PromptPetrol therefore needs a periodic authoritative
reconciliation even when native notifications are active.

Sources: [`notify` known problems](https://docs.rs/notify/latest/notify/#known-problems),
[`Event::need_rescan`](https://docs.rs/notify/latest/notify/struct.Event.html#method.need_rescan)

### Terminal Lifecycle

Ratatui 0.30 initialization already installs a panic hook that restores terminal
state. The current code manually enables raw mode and enters the alternate
screen before calling `ratatui::init()`. The implementation task is to simplify
this lifecycle around `ratatui::try_init`/`try_restore` or `ratatui::run`, then
test it; adding another independent panic hook is unnecessary.

Source: [Ratatui initialization and panic-hook guidance](https://docs.rs/ratatui/0.30.2/ratatui/init/index.html)

### Persistence

SQLite is appropriate once durable ingest state or history is approved because
it provides transactional writes, schema constraints, and migrations in one
local file. WAL is not an automatic requirement: it adds checkpoint behavior
and companion files. Enable it only if measured concurrent read/write behavior
justifies it, and keep the database with its WAL during backup or copying.

Sources: [SQLite WAL](https://www.sqlite.org/wal.html),
[SQLite constraints](https://www.sqlite.org/lang_createtable.html)

### Benchmarks And Credentials

Use Criterion for local statistical comparisons across input sizes. Avoid
failing ordinary cloud CI on noisy microbenchmark deltas; retain deterministic
smoke ceilings there. For cross-platform credentials, select explicit native
stores rather than pulling every keyring backend into the binary.

Sources: [Criterion guide](https://bheisler.github.io/criterion.rs/book/),
[Criterion CI caveat](https://bheisler.github.io/criterion.rs/book/faq.html#how-should-i-run-criterionrs-benchmarks-in-a-ci-pipeline),
[`keyring` platform stores](https://docs.rs/keyring/latest/keyring/cli/index.html)

## Target Runtime

```text
                         bounded commands
TUI/event loop  --------------------------------->  Runtime coordinator
     ^                                                    |
     | small immutable UiSnapshot                         |
     +----------------------------------------------------+
                                                          |
                              +---------------------------+------------------+
                              |                                              |
                       Codex worker                                   Claude worker
                  owns index + watcher                           owns Client + token
                              |                                              |
                        ProviderSnapshot                              ProviderSnapshot
                              +-------------------+--------------------------+
                                                  |
                                           snapshot reducer
```

Use standard threads and bounded channels first. Two blocking sources do not
justify an async-runtime migration. A coordinator can start and supervise one
long-lived worker per source; each worker owns its mutable resources. The UI
keeps only the most recent small normalized snapshot.

### Domain Boundary

```rust
struct ProviderSnapshot {
    provider_id: ProviderId,
    windows: Vec<UsageWindow>,
    context: Option<ContextWindow>,
    health: SourceHealth,
}

struct UsageWindow {
    window_id: String,
    label: String,
    used_percent: f64,
    resets_at: Option<DateTime<Utc>>,
    observed_at: DateTime<Utc>,
    source_timestamp: Option<DateTime<Utc>>,
}
```

The concrete implementation may use fixed arrays for the current two windows,
but the UI must not import `CodexRateLimits` or importer cache types. Validate
percentages as finite and nonnegative at the boundary; preserve over-100 values
for diagnostics instead of silently treating them as normal.

### Source Health Contract

Every source publishes:

- `last_attempt_at`
- `last_success_at`
- `source_timestamp`
- `last_duration`
- `consecutive_failures`
- `next_retry_at`
- categorized error without secrets
- state: `Acquiring`, `Live`, `Stale`, `RateLimited`, `AuthFailed`, `Offline`, or `Disabled`

Transient failures retain the last successful metrics but change their state and
age. Authentication failures never trigger rapid automatic retries. Manual
refresh bypasses the normal cadence once but still respects an active server
`Retry-After` deadline.

## Incremental JSONL Rules

The cursor is committed only through the last newline-delimited record that was
fully read. A partial final line is retained and retried after the next append.

For every session store:

- canonical path
- file identity when available
- last observed length and modification time
- committed byte offset
- partial trailing bytes
- latest accepted token and limit state
- compact fingerprint for replacement detection

Perform a full per-file rebuild when length decreases, identity changes, the
source directory changes, or cursor validation fails. Watcher events enqueue
paths; they never directly mutate state. Event overflow or `need_rescan()`
requests a complete reconciliation.

## History Contract

History is a later phase because forecast correctness depends on stable snapshot
semantics. When introduced:

- Record normalized percentages and source health, never raw prompts or JSONL.
- Insert on meaningful value/state change plus a low-frequency heartbeat.
- Use a unique source/window/observation identity to reject duplicates.
- Separate reset windows so burn-rate calculations never cross a reset.
- Suppress forecasts when timestamps regress or sample coverage is insufficient.
- Apply bounded retention and expose database size/checkpoint health.

Billing data, if approved, gets separate types and tables. Subscription-window
utilization must not be presented as token cost.

## Provisional Performance Budgets

Ratify these after S0 measurements on a named reference machine and dataset.

| Path | Provisional budget |
|------|--------------------|
| Render cached 120x20 or 120x40 snapshot | p95 < 5 ms |
| Enqueue refresh command on UI thread | p95 < 1 ms |
| Warm no-change refresh at 10,000 files | p95 < 10 ms, no full-tree metadata pass |
| Parse a 64 KiB append | p95 < 20 ms |
| Discover 10,000 local files | p95 < 100 ms on reference APFS host |
| Cold parse 100 MiB fixture corpus | < 2 s on reference host |
| Repeated 1,000 refresh cycle | no unbounded memory or thread growth |

Measure UI latency separately from worker throughput. A fast background parse
does not compensate for cache cloning or traversal performed before enqueueing.

## Delivery Phases

### Phase S0 - Baseline And Contracts

Build representative fixtures and benchmarks before changing architecture.
Freeze the normalized data and source-health semantics.

### Phase S1 - Source Trust

Add freshness, categorized failure handling, HTTP tests, retry policy, monotonic
scheduling, and verified terminal lifecycle.

### Phase S2 - Runtime Boundary

Move to immutable UI snapshots and persistent cache-owning workers. Precompute
latest context/limit views and make initial acquisition asynchronous.

### Phase S3 - Incremental Ingest

Add append cursors, replacement recovery, watcher acceleration, reconciliation,
and eventually a durable ingest index.

### Phase S4 - History And Forecasting

Add SQLite only after snapshot semantics are stable. Prove migration,
deduplication, retention, reset handling, and forecast confidence.

### Phase S5 - MFD Pages And Alerts

Add Summary, Trends, and Data Link pages. Use threshold hysteresis and cooldown;
do not emit notifications every refresh.

### Phase S6 - Expansion

Add a provider adapter abstraction when a third real provider is implemented.
Add remote/team operation only after local persistence, credentials, and threat
model are complete.

## Explicit Non-Goals For S0-S3

- Hosted or multi-tenant service
- Web dashboard
- Plugin ABI
- Remote collection agents
- Billing/cost estimation
- Raw prompt or conversation storage
- Tokio migration without measured need

## Definition Of Done

The scaling milestone is complete when:

1. The UI performs no work proportional to historical session count.
2. Every displayed metric exposes source freshness and failure state.
3. Codex append refresh cost scales with appended bytes.
4. Watcher failure and event loss recover through reconciliation.
5. Claude retry behavior is deterministic, bounded, and server-aware.
6. Clean full parse, incremental parse, and restart recovery yield identical snapshots.
7. The 10,000-file and 100 MiB profiles meet ratified budgets.
8. No raw session content or credentials enter history, logs, fixtures, or diagnostics.

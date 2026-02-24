# TODO

- [x] Define MVP scope for PromptPetrol TUI
- [x] Initialize Rust project with `cargo`
- [x] Choose TUI stack (`ratatui` + `crossterm`)
- [x] Design terminal layout (usage, cost, alerts, trends)
- [x] Build baseline token usage ingestion pipeline (JSON file)
- [x] Implement local persistence (JSON)
- [x] Create interactive dashboard for usage and cost tracking
- [x] Add budget alerts and threshold indicators
- [x] Add realtime refresh loop for live token monitoring
- [x] Add dynamic layout handling for variable terminal columns and rows
- [x] Add provider adapters for OpenAI, Codex, Opus, Anthropic, Gemini, and other model providers
- [x] Normalize provider usage into a common token/cost schema
- [x] Add config support for API keys and model pricing
- [x] Write setup and usage documentation

- [x] Live codex limit tracking is not working
- [x] Live tracking should refresh every 10 seconds i.e 0.1hz

## Next improvements

- [x] Split `src/main.rs` into modules (`app`, `ui`, `codex_import`, `models`) to reduce coupling and improve testability
- [x] Add integration tests for Codex session import using fixture `.jsonl` files (including malformed lines and mixed event types)
- [x] Add a large-session performance pass (benchmark + optimize recursive scan of `~/.codex/sessions`)
- [x] Add optional debounce/backoff for auto-refresh when no files changed for extended periods
- [x] Surface Codex import diagnostics in the UI status line (sessions read, parse failures, last import time)
- [x] Add keyboard shortcut help panel in-app (toggle with `?`)
- [x] Add CLI flags for `--data-file`, `--config-file`, and `--refresh-interval-seconds`
- [x] Add export command for provider summaries (`json` and `csv`)
- [x] Add CI workflow for `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test`
- [x] Expand README with troubleshooting for Codex import paths, permissions, and stale session data

## Scalability & future scope

### P0 - Correctness and observability

- [x] Split Codex diagnostics into `parse_error_files`, `no_usage_or_limits_files`, and `unreadable_files`
- [x] Add unit tests for diagnostics categorization and edge timestamp cases
- [x] Add staleness/freshness indicator in UI info/alerts for Codex snapshot age

### P1 - Parsing performance

- [x] Replace full-file `read_to_string` with streaming JSONL parsing (`BufRead`)
- [ ] Introduce typed partial structs for Codex events instead of generic `serde_json::Value`
- [ ] Parse only relevant event types quickly (`session_meta`, `event_msg/token_count`)
- [ ] Add benchmarks for 10k+ files and very large session files

### P2 - Incremental ingest architecture

- [ ] Add per-session cursor/index cache (`mtime`, `len`, `last_offset`, `last_event_ts`)
- [ ] Implement append-only incremental parsing for changed files
- [ ] Persist ingest index to disk to reduce cold-start rescans
- [ ] Ensure stable dedupe/merge logic across repeated reload cycles

### P3 - Discovery scalability

- [ ] Add optional filesystem watcher mode (`notify`) for session discovery
- [ ] Keep recursive scan fallback when watcher is unavailable
- [ ] Tune backoff strategy using activity and watcher health

### P4 - UI responsiveness and growth

- [ ] Move import/parse work off the UI thread into a background worker
- [ ] Render from immutable snapshots to avoid frame stalls
- [ ] Add Codex trend mini-panel (5h/weekly history) once incremental ingest lands
- [ ] Add pagination/virtualization if activity lists grow significantly

## OpenAI-Codex usage/limit parity plan

### P0 - Correct provider/model identity for Codex sessions

- [ ] Parse and persist provider/model identity from Codex session metadata (e.g. `model_provider` and any model identifier fields)
- [ ] Stop hard-forcing imported entries to `provider = "codex"`; map to actual provider when available (e.g. `openai`)
- [ ] Replace default model fallback `"codex-cli"` with a more explicit fallback strategy (e.g. `unknown-codex-model`) and surface fallback use in diagnostics
- [ ] Add tests with fixtures covering: provider present, model present, provider only, model only, neither present

### P1 - Capture full token accounting from Codex events

- [ ] Extend Codex parser to read additional token fields beyond input/output (at minimum: `cached_input_tokens`, `reasoning_output_tokens`, and total token fields when present)
- [ ] Define a clear normalization policy for totals (what counts as `total_tokens`, and how cached/reasoning are represented)
- [ ] Update `UsageEntry`/aggregation to preserve both compatibility totals and expanded token breakdown
- [ ] Add regression tests proving totals match real Codex JSONL samples with mixed token payload shapes (`info = null`, partial usage, full usage)

### P2 - Show limit usage for codex-derived models, not only `provider == "codex"`

- [ ] Decouple limit UI gating from strict provider string check; enable Codex limit widgets when selected provider/model is backed by Codex session import
- [ ] Add explicit source metadata on imported entries so UI can reliably detect Codex-origin records even if provider is `openai`
- [ ] Keep graceful fallback when no limits are available (`rate_limits` missing/null)
- [ ] Add UI tests/snapshots for both cases: codex-origin OpenAI model and non-codex OpenAI model

### P3 - Diagnostics, migration, and UX clarity

- [ ] Extend status line diagnostics to include: number of codex-origin entries, entries using fallback provider/model, and last limits timestamp
- [ ] Add a one-time migration note in README/changelog explaining provider/model identity behavior change
- [ ] Document token accounting semantics (input/output vs cached/reasoning) in README with examples from real Codex payloads
- [ ] Add troubleshooting steps for “limits visible but token totals missing” and “token totals visible but limits missing”

### P4 - Verification checklist (for PR acceptance)

- [ ] Validate against real local `~/.codex/sessions` sample set (including sessions where `info` alternates between null and populated)
- [ ] Confirm `openai-codex`-style sessions show both token usage and 5h/weekly limits in the dashboard
- [ ] Run full quality gate: `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test`
- [ ] Include before/after screenshots (or text snapshots) proving the fix for provider/model naming, token totals, and limit visibility

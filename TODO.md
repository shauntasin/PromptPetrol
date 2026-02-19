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

- [ ] Replace full-file `read_to_string` with streaming JSONL parsing (`BufRead`)
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

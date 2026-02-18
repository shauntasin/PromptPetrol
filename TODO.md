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
- [ ] Add a large-session performance pass (benchmark + optimize recursive scan of `~/.codex/sessions`)
- [ ] Add optional debounce/backoff for auto-refresh when no files changed for extended periods
- [ ] Surface Codex import diagnostics in the UI status line (sessions read, parse failures, last import time)
- [ ] Add keyboard shortcut help panel in-app (toggle with `?`)
- [ ] Add CLI flags for `--data-file`, `--config-file`, and `--refresh-interval-seconds`
- [ ] Add export command for provider summaries (`json` and `csv`)
- [ ] Add CI workflow for `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test`
- [ ] Expand README with troubleshooting for Codex import paths, permissions, and stale session data

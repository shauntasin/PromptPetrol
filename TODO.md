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

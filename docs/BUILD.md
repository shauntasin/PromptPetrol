# Build & Development Guide

## Prerequisites

- **Rust** (edition 2024) — install via [rustup](https://rustup.rs/)
- **macOS Keychain** (optional) — for auto-detecting Claude OAuth token

## Quick Start

```bash
cargo run
```

## Build Commands

| Command | Description |
|---------|-------------|
| `cargo build` | Debug build |
| `cargo build --release` | Optimized release build |
| `cargo run` | Run the application |
| `cargo run -- --config-file /path/to/config.json` | Run with custom config |
| `cargo run -- --refresh-interval-seconds 5` | Run with custom refresh rate |

## CLI Flags

```
--config-file <PATH>            Path to config.json (default: ~/.config/promptpetrol/config.json)
--refresh-interval-seconds <N>  Data refresh interval in seconds (default: 10, accepts fractions like 0.5)
```

## Testing

```bash
cargo test --all-targets
```

### Test Structure

| Module | Tests | Coverage |
|--------|-------|----------|
| `ui.rs` | 9 tests | Rendering at multiple terminal sizes, drum alignment, full and medium MFD layouts |
| `codex_import.rs` | 15 tests | JSONL parsing, fixture integration, cache transitions, backoff, diagnostics |
| `app.rs` | 3 tests | Config reload authority, invalid config retention, background refresh |
| `models.rs` | 2 tests | Nested config bootstrap and private Unix permissions |
| `main.rs` | 3 tests | CLI parsing, help/version, invalid intervals |
| `claude_import.rs` | 1 test | Disabled-import state reset |

### Test Fixtures

Located in `tests/fixtures/codex/`:

- `mixed_usage_and_limits.jsonl` — normal mixed events
- `limits_only_malformed.jsonl` — limits without usage, some malformed lines
- `no_token_or_limits_mixed.jsonl` — no token data, mixed event types

### Ignored Tests

Some tests are marked `#[ignore]` for visual inspection or performance benchmarking:

```bash
cargo test -- --ignored          # run ignored tests
cargo test -- --include-ignored  # run all tests
```

## Code Quality

```bash
cargo fmt --check                  # check formatting
cargo fmt                          # auto-format
cargo clippy --all-targets -- -D warnings   # lint (warnings are errors)
```

## CI Pipeline

Defined in `.github/workflows/ci.yml`. Runs on push to `main` and all PRs:

1. `cargo fmt --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test --all-targets`

Uses `dtolnay/rust-toolchain@stable` with `rustfmt` and `clippy` components, plus `Swatinem/rust-cache@v2` for dependency caching.

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `ratatui` | 0.30.2 | TUI framework (layout, widgets, canvas) |
| `crossterm` | 0.29.0 | Terminal raw mode, key events, alternate screen |
| `serde` | 1.0.228 | Serialization (with `derive` feature) |
| `serde_json` | 1.0.149 | JSON parsing |
| `reqwest` | 0.12 | HTTP client (with `blocking`, `json` features) |
| `chrono` | 0.4 | DateTime handling |
| `color-eyre` | 0.6.5 | Error reporting |
| `dirs` | 6.0.0 | Cross-platform config directory resolution |

## Project Layout

```
PromptPetrol/
├── Cargo.toml              # Package manifest
├── Cargo.lock              # Dependency lockfile
├── README.md               # User documentation
├── TODO.md                 # Roadmap and task tracking
├── .github/workflows/
│   └── ci.yml              # CI pipeline
├── src/
│   ├── main.rs             # Entry point, CLI parsing
│   ├── app.rs              # App state, event loop, terminal init
│   ├── models.rs           # Config structs, file loading
│   ├── ui.rs               # TUI rendering (MFD, compact bars, help)
│   ├── codex_import.rs     # Codex JSONL parser, cache, discovery
│   └── claude_import.rs    # Claude OAuth fetch, keychain detect
├── tests/fixtures/codex/   # Test fixture JSONL files
└── docs/                   # This documentation
```

## Runtime Files

PromptPetrol creates `~/.config/promptpetrol/config.json` on first run. It does
not currently create or consume a usage-history file.

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `q` | Quit |
| `r` | Force reload usage data and config |
| `?` | Toggle keyboard help overlay |

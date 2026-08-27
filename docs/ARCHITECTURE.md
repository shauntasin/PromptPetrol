# Architecture

PromptPetrol is a Rust TUI application that monitors Claude and Codex subscription utilization, rendered as an avionics multi-function display.

## High-Level Overview

```
┌─────────────────────────────────────────────────────┐
│                      main.rs                        │
│         CLI parsing → bootstrap → event loop         │
└────────────┬──────────────────────┬─────────────────┘
             │                      │
             ▼                      ▼
      ┌─────────────┐      ┌──────────────┐
      │   app.rs    │      │   models.rs  │
      │ App state,  │      │ Config load, │
      │ event loop  │      │ bootstrap    │
      └──────┬──────┘      └──────────────┘
             │
      ┌──────┼──────────────────┐
      │      │                  │
      ▼      ▼                  ▼
┌──────────┐ ┌──────────────┐ ┌──────────────────┐
│  ui.rs   │ │codex_import  │ │ claude_import.rs │
│  TUI     │ │   .rs        │ │  OAuth fetch,    │
│  render  │ │ JSONL parse, │ │  keychain detect │
│          │ │ cache, scan  │ │                  │
└──────────┘ └──────────────┘ └──────────────────┘
```

## Module Breakdown

### `main.rs` (entry point)

- Parses CLI arguments: `--config-file`, `--refresh-interval-seconds`
- Installs `color_eyre` error handler
- Calls `bootstrap_app()` → `init_terminal()` → `run()` → `restore_terminal()`
- No business logic; delegates everything to other modules

### `app.rs` (application core)

- **`App` struct** — holds all runtime state:
  - `config: AppConfig` — loaded from `config.json`
  - `config_file: PathBuf` — authoritative config path across reloads
  - `config_error: Option<String>` — most recent config reload failure
  - `codex_cache: CodexImportCache` — parsed Codex sessions + rate limits
  - `claude_cache: ClaudeImportDiagnostics` — fetched Claude usage + limits
  - `show_help: bool` — help overlay toggle
- **`App::request_reload()`** — starts a single background refresh. Additional requests are coalesced while work is in flight.
- **`App::poll_reload()`** — atomically applies a completed config/data snapshot without blocking rendering.
- **`run()`** — main event loop:
  - Renders at 2 Hz (500ms tick) via `RENDER_INTERVAL`
  - Background data refresh defaults to 10s via `DEFAULT_REFRESH_INTERVAL` (configurable)
  - Handles key events: `q` (quit), `r` (reload), `?` (help toggle)
- **Terminal lifecycle**: `init_terminal()` / `restore_terminal()` manage raw mode and alternate screen

### `models.rs` (configuration)

- **`AppConfig`** — top-level config struct with Codex/Claude enable switches and `claude_oauth_token`
- **`CodexImportConfig`** — `enabled` flag + optional `sessions_dir` override
- **`load_or_bootstrap_config()`** — reads existing config or securely creates a default one (mode `0600` on Unix)
- Uses `dirs::config_dir()` for cross-platform config path (`~/.config/promptpetrol/`)
- Unknown fields are ignored for backward compatibility

### `ui.rs` (rendering)

- **`draw()`** — top-level render dispatch:
  - Terminals ≥ 90×24: renders the full avionics MFD (`render_cluster()`)
  - Terminals ≥ 90×16: renders the medium avionics MFD (`render_medium_cluster()`)
  - Smaller terminals: renders compact dashboard (`render_dashboard()`)
  - Terminals < 24×6: shows "enlarge terminal" message
- **Avionics MFD** — high-density instrument panel:
  - Claude and Codex provider bays with large digital readouts and calibrated load tapes
  - Center resource scope with a circular utilization ring, reserve count, and state annunciator
  - Header and footer bands for data-link state, UTC, diagnostics, and control legends
  - Medium mode retains the three-bay layout with compact tapes and status rails at 120×20
- **Compact dashboard** — fallback for smaller terminals:
  - Two-column odometer bars (Claude left, Codex right)
  - Single-column fallback for very narrow terminals
  - Full-width context bar at bottom when data available
- **Color coding**: Murphy green (< 70%), orange (70–90%), red (> 90%)
- **Theme**: Neovim Murphy true-color palette with green, cyan, yellow, orange,
  magenta, white, dark-green, and gray semantic tokens
- **Help overlay**: centered modal with keyboard shortcuts

### `codex_import.rs` (Codex session parser)

- **File discovery**: recursively scans `~/.codex/sessions` (or custom path) for `.jsonl` files
- **Streaming parser**: `BufRead`-based line-by-line parsing (not full-file `read_to_string`)
- **Typed structs**: `CodexSessionLine`, `CodexSessionLinePayload`, `CodexTokenInfo`, etc.
- **Caching**: tracks `mtime` + `file_len` per session file; only re-parses changed files
- **Discovery backoff**: scans every 10s initially; backs off to 120s after 3 idle cycles; resets to 10s when changes detected
- **Diagnostics**: tracks `active_files`, `refreshed_files`, `parse_error_files`, `no_usage_or_limits_files`, `unreadable_files`
- **Context window**: calculated from latest session as `input - cached_input + output` against `model_context_window`
- **Rate limits**: extracted from `token_count` events with `rate_limits` payload; supports both `f64` and integer `used_percent` fields

### `claude_import.rs` (Claude OAuth integration)

- **OAuth token resolution**:
  1. `claude_oauth_token` in `config.json` (if set and non-empty)
  2. Auto-detected from macOS Keychain (`Claude Code-credentials` entry)
- **API endpoint**: `GET https://api.anthropic.com/api/oauth/usage` with Bearer token
- **Response**: `five_hour` and `seven_day` utilization percentages + reset timestamps
- **Keychain detection**: runs `security find-generic-password -s "Claude Code-credentials" -w`, parses JSON for `claudeAiOauth.accessToken`, validates `sk-ant-oat` prefix
- **Error handling**: surfaces auth failures (401/403) and network errors in the UI status line

## Data Flow

```
1. Background refresh worker
   ├─ re-read config.json (if changed)
   ├─ merge_codex_usage()
   │   ├─ discover .jsonl files (with backoff)
   │   ├─ parse changed files (streaming BufRead)
   │   ├─ update session cache
   │   ├─ find latest rate limits
   │   └─ update diagnostics
   └─ merge_claude_usage()
       ├─ resolve OAuth token (config → keychain)
       ├─ fetch /api/oauth/usage
       └─ update diagnostics + limits

2. App::poll_reload() atomically installs the completed snapshot

3. draw(frame, app)
   ├─ collect_metrics() → Claude + Codex metric structs
   ├─ collect_context() → context window snapshot
   ├─ if wide + tall: render_cluster() (full avionics MFD)
   ├─ if wide + shallow: render_medium_cluster() (medium avionics MFD)
   └─ otherwise: render_dashboard() (compact odometers)
```

## Timing Model

| Component | Interval | Purpose |
|-----------|----------|---------|
| Render loop | 500ms (2 Hz) | UI redraw from cached state |
| Data refresh | 10s (configurable) | Network fetch + filesystem scan |
| Discovery scan | 10s–120s (adaptive) | File discovery in `~/.codex/sessions` |

The render and data loops are decoupled. Filesystem and network work runs on a
single-flight background thread, so a slow request does not stall input or paint
cycles and repeated refresh requests cannot create overlapping API calls.

## Key Types

| Type | Module | Purpose |
|------|--------|---------|
| `App` | `app.rs` | Runtime state container |
| `AppConfig` | `models.rs` | Deserialized config |
| `CodexImportCache` | `codex_import.rs` | Session file cache + limits |
| `CodexSessionSnapshot` | `codex_import.rs` | Latest session token counts |
| `ClaudeImportDiagnostics` | `claude_import.rs` | Claude fetch state + limits |
| `Metric` | `ui.rs` | Single limit instrument readout |
| `Context` | `ui.rs` | Context window bar data |

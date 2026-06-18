# PromptPetrol

PromptPetrol is a Rust TUI app for monitoring AI token usage like fuel usage.

## Features

- Terminal dashboard for total tokens and spend.
- Budget burn gauge with threshold coloring.
- Recent usage activity list.
- JSON-backed local storage.
- Provider adapters for OpenAI, Codex, Opus, Anthropic, Gemini, and generic formats.
- Normalization into a common `input_tokens` / `output_tokens` / `cost_usd` schema.
- Automatic Codex CLI usage import from `~/.codex/sessions` (cached for fast refresh).
- Live Claude usage import via the Claude Code OAuth token (5-hour and weekly limits).
- Automotive-style instrument cluster: analog dials (BMW-like) for the 5-hour and
  weekly limits with needles, tick arcs, and redline zones, plus a center
  context-window readout. UI redraws at 2 Hz; data refreshes every ~10s.
- Responsive layout: the dial cluster renders on large terminals (about 74x20 and
  up); smaller terminals fall back to compact odometer bars, then to a single line.
- Config-driven API keys and model pricing for cost estimation.

## Run

```bash
cargo run
```

Optional flags:

```bash
cargo run -- \
  --config-file /path/to/config.json \
  --refresh-interval-seconds 10   # accepts fractional seconds, e.g. 0.5
```

The UI redraws at 2 Hz for responsiveness; `--refresh-interval-seconds` controls
how often the underlying data (Claude network fetch + Codex session scan) is
re-read. These are decoupled so a fast UI does not spam the Claude API.

## Controls

- `q`: quit
- `r`: reload usage data and config from disk
- `?`: toggle keyboard help panel

## Data file

On first run, PromptPetrol creates:

- macOS/Linux: `~/.config/promptpetrol/usage.json`
- macOS/Linux: `~/.config/promptpetrol/config.json`

Example format:

```json
{
  "budget_usd": 50.0,
  "entries": [
    {
      "timestamp": "2026-02-10T03:15:00Z",
      "provider": "openai",
      "model": "gpt-4.1-mini",
      "input_tokens": 5300,
      "output_tokens": 1200,
      "cost_usd": 0.056
    }
  ]
}
```

## Config file

`config.json` includes:

- `api_keys`: provider key map (for local configuration only)
- `pricing`: map of `"provider/model"` to per-million token rates

If a usage entry is missing `cost_usd`, PromptPetrol estimates it from pricing.

Example:

```json
{
  "api_keys": {
    "openai": "<set-openai-key>",
    "anthropic": "<set-anthropic-key>"
  },
  "pricing": {
    "openai/gpt-4.1-mini": {
      "input_per_million_usd": 0.4,
      "output_per_million_usd": 1.6
    },
    "anthropic/*": {
      "input_per_million_usd": 3.0,
      "output_per_million_usd": 15.0
    }
  },
  "codex_import": {
    "enabled": true,
    "sessions_dir": null,
    "model": "codex-cli"
  }
}
```

## Codex usage import

When `codex_import.enabled` is true, PromptPetrol reads Codex session `.jsonl` files from:

- Default: `~/.codex/sessions`
- Or custom: `codex_import.sessions_dir`

PromptPetrol uses the latest `token_count` totals found in each session file and adds them as `provider = "codex"` entries in the dashboard.
It also shows Codex rate-limit usage (5-hour and weekly) and the current context-window
fill, taken from the most recent session, when available in session events.

> Context-window fill uses the latest session's *fresh* tokens
> (`input − cached_input + output`) against `model_context_window`, since Codex
> reports `input_tokens` cumulatively across turns.

## Claude usage import

PromptPetrol shows live Claude subscription usage (5-hour and weekly limits with
reset countdowns) by querying the Claude OAuth usage endpoint. The OAuth token is
resolved in this order:

1. `claude_oauth_token` in `config.json`, if set and non-empty.
2. Auto-detected from the macOS Keychain entry that Claude Code stores
   (`Claude Code-credentials`).

The endpoint reports a utilization percentage per window (not raw token counts),
so the Claude panel displays percentages plus reset times. If the token is missing
or rejected, the status line shows the reason (e.g. `No OAuth token` or
`Auth failed (401/403)`).

## Troubleshooting Codex import

- Confirm `codex_import.enabled` is `true` in `config.json`.
- If you use a non-default Codex sessions path, set `codex_import.sessions_dir`.
- If limits/usage look stale, press `r` to force a reload and check the Info line for:
  - files discovered,
  - refreshed session files,
  - parse failures,
  - current scan interval.
- Parse failures usually indicate malformed or partial `.jsonl` lines; PromptPetrol ignores bad lines but counts failed files in diagnostics.
- Discovery scans back off when no files change, then reset to fast scan when activity resumes. Use `--refresh-interval-seconds` to tune UI refresh cadence.

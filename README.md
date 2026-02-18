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
- Config-driven API keys and model pricing for cost estimation.

## Run

```bash
cargo run
```

Optional flags:

```bash
cargo run -- \
  --data-file /path/to/usage.json \
  --config-file /path/to/config.json \
  --refresh-interval-seconds 10
```

Export provider summaries without opening the TUI:

```bash
cargo run -- --export-json /tmp/promptpetrol-summary.json
cargo run -- --export-csv /tmp/promptpetrol-summary.csv
```

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
It also shows Codex rate-limit usage in Alerts (5-hour and weekly) when available in session events.

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

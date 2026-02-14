# PromptPetrol

PromptPetrol is a Rust TUI app for monitoring AI token usage like fuel usage.

## Features

- Terminal dashboard for total tokens and spend.
- Budget burn gauge with threshold coloring.
- Recent usage activity list.
- JSON-backed local storage.
- Provider adapters for OpenAI, Codex, Opus, Anthropic, Gemini, and generic formats.
- Normalization into a common `input_tokens` / `output_tokens` / `cost_usd` schema.
- Config-driven API keys and model pricing for cost estimation.

## Run

```bash
cargo run
```

## Controls

- `q`: quit
- `r`: reload usage data and config from disk

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
  }
}
```

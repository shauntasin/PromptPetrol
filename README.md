# PromptPetrol

PromptPetrol is a Rust TUI app for monitoring AI token usage like fuel usage.

## Current MVP

- Terminal dashboard for total tokens and spend.
- Budget burn gauge with threshold coloring.
- Recent usage activity list.
- JSON-backed local data storage.

## Run

```bash
cargo run
```

## Controls

- `q`: quit
- `r`: reload usage data from disk

## Data file

On first run, PromptPetrol creates:

- macOS/Linux: `~/.config/promptpetrol/usage.json`

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

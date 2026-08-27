# Data Formats

## Config File (`~/.config/promptpetrol/config.json`)

Created automatically on first run. Unknown fields are ignored for backward compatibility.

### Current Schema

```json
{
  "codex_import": {
    "enabled": true,
    "sessions_dir": null
  },
  "claude_import": {
    "enabled": true
  },
  "claude_oauth_token": null
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `codex_import.enabled` | `bool` | `true` | Enable/disable Codex session import |
| `codex_import.sessions_dir` | `string \| null` | `null` | Custom path to Codex sessions directory (default: `~/.codex/sessions`) |
| `claude_import.enabled` | `bool` | `true` | Enable/disable Keychain discovery and Claude API fetching |
| `claude_oauth_token` | `string \| null` | `null` | Claude OAuth token (if not set, auto-detected from macOS Keychain) |

### Legacy Fields (still accepted, ignored)

Older configs may include `api_keys` and `pricing` maps. These are harmless and silently ignored.

```json
{
  "api_keys": {
    "openai": "...",
    "anthropic": "..."
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

## Legacy Usage File

Older project plans described `~/.config/promptpetrol/usage.json` using the
format below. The current application neither creates nor reads this file.

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

| Field | Type | Description |
|-------|------|-------------|
| `budget_usd` | `number` | Monthly budget limit |
| `entries` | `array` | List of usage records |
| `entries[].timestamp` | `string` | ISO 8601 timestamp |
| `entries[].provider` | `string` | Provider name (e.g. `openai`, `codex`) |
| `entries[].model` | `string` | Model identifier |
| `entries[].input_tokens` | `number` | Input token count |
| `entries[].output_tokens` | `number` | Output token count |
| `entries[].cost_usd` | `number` | Cost in USD (estimated from pricing if missing) |

## Codex Session Files (`~/.codex/sessions/**/*.jsonl`)

Each file is a JSONL (one JSON object per line) representing a Codex session.

### Event Types

#### `session_meta`

```json
{
  "timestamp": "2026-02-16T09:45:42.927Z",
  "type": "session_meta",
  "payload": {
    "timestamp": "2026-02-16T09:45:42.927Z"
  }
}
```

#### `event_msg` with `token_count`

```json
{
  "timestamp": "2026-02-16T09:45:56.220Z",
  "type": "event_msg",
  "payload": {
    "type": "token_count",
    "info": {
      "total_token_usage": {
        "input_tokens": 17438,
        "output_tokens": 326,
        "cached_input_tokens": 1200
      },
      "model_context_window": 258400
    },
    "rate_limits": {
      "primary": {
        "used_percent": 7.0,
        "window_minutes": 300,
        "resets_at": 1771243734
      },
      "secondary": {
        "used_percent": 25.0,
        "window_minutes": 10080,
        "resets_at": 1771317088
      }
    }
  }
}
```

### Parsing Rules

1. **Session timestamp**: taken from `session_meta` payload timestamp or line-level timestamp
2. **Token usage**: accumulated from `token_count` events with `info.total_token_usage`
3. **Context window**: read from `info.model_context_window` (last seen wins)
4. **Rate limits**: last `rate_limits` payload in the file wins
5. **`used_percent`**: accepts both `f64` and `u64` (integer) — normalized to `f64`
6. **`info: null`**: rate limits can still be present even when `info` is null
7. **Malformed lines**: silently skipped; a file is classified as a parse error only when it contains no valid JSON lines

### Context Window Calculation

```
fresh_tokens = input_tokens - cached_input_tokens + output_tokens
context_percent = fresh_tokens / model_context_window × 100
```

Uses only the latest session's fresh tokens because Codex reports `input_tokens` cumulatively across turns.

## Claude OAuth Response

Endpoint: `GET https://api.anthropic.com/api/oauth/usage`

Headers:
- `Authorization: Bearer <token>`
- `anthropic-beta: oauth-2025-04-20`
- `User-Agent: claude-code/0.1`

Response:

```json
{
  "five_hour": {
    "utilization": 42.0,
    "resets_at": "2026-02-16T14:00:00Z"
  },
  "seven_day": {
    "utilization": 15.0,
    "resets_at": "2026-02-22T00:00:00Z"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `five_hour.utilization` | `number` | 5-hour window usage percentage |
| `five_hour.resets_at` | `string \| null` | ISO 8601 reset timestamp |
| `seven_day.utilization` | `number` | Weekly window usage percentage |
| `seven_day.resets_at` | `string \| null` | ISO 8601 reset timestamp |

## Internal Types

### `CodexRateLimits`

Used by both Codex and Claude data paths:

```rust
struct CodexRateLimits {
    timestamp: String,
    primary: Option<CodexRateLimit>,   // 5-hour window
    secondary: Option<CodexRateLimit>, // weekly window
}

struct CodexRateLimit {
    used_percent: f64,
    resets_at: Option<u64>,  // epoch seconds
}
```

### `CodexSessionSnapshot`

Latest session token figures for the context window gauge:

```rust
struct CodexSessionSnapshot {
    latest_input: u64,
    latest_output: u64,
    latest_cached: u64,
    latest_context_window: u64,
}
```

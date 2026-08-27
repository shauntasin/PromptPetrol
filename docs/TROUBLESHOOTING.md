# Troubleshooting

## Codex Import

### "No Codex data showing"

1. Confirm `codex_import.enabled` is `true` in `~/.config/promptpetrol/config.json`
2. Verify `~/.codex/sessions` contains `.jsonl` files
3. If sessions are in a custom location, set `codex_import.sessions_dir` in config
4. Press `r` to force a reload

### "Usage looks stale"

- Press `r` to force a reload
- Check the bottom border for diagnostics:
  - `files discovered` — how many `.jsonl` files were found
  - `refreshed files` — how many were re-parsed (changed mtime/len)
  - `errors` — malformed or unreadable files encountered in the latest refresh
  - `scan interval` — current discovery interval (backs off when idle)
- Discovery scans back off after 3 idle cycles (10s → 20s → ... → 120s)
- When new activity is detected, scan interval resets to 10s

### "Context window shows -- or 0%"

- Only the latest session's token data is used for context
- The session must have `info.total_token_usage` with non-zero tokens
- The session must have `info.model_context_window` > 0
- `input_tokens` is cumulative across turns; only `input - cached + output` is counted as fresh

### "Rate limits show --"

- Rate limits are extracted from `token_count` events with a `rate_limits` payload
- If no events contain rate limits, the instrument shows `--`
- Rate limits from the most recently modified session file are used

### "Parse failures in diagnostics"

- Parse failures indicate malformed or partial `.jsonl` lines
- PromptPetrol ignores bad lines and continues parsing
- Common causes: truncated files, non-JSON content, binary corruption
- The file is skipped but counted in diagnostics

## Claude Import

### "No OAuth token"

The status line shows `No OAuth token` when:

1. `claude_oauth_token` is not set in `config.json`, AND
2. macOS Keychain auto-detection failed

**Fix**: Either set `claude_oauth_token` in config, or ensure Claude Code has been authenticated at least once (creates the `Claude Code-credentials` Keychain entry).

### "Auth failed (401/403)"

The OAuth token was found but rejected by the API.

**Fix**: The token may have expired. Re-authenticate with Claude Code to refresh it, or set a fresh token in `config.json`.

### "Fetch error"

Network-level failure reaching `api.anthropic.com`.

**Fix**: Check network connectivity. The request times out after 5 seconds.

### Keychain detection fails on Linux

The `security find-generic-password` command is macOS-only. On Linux, you must set `claude_oauth_token` explicitly in `config.json`.

## UI Issues

### "Terminal too small"

The full MFD requires at least 90×24 drawable characters. The UI degrades gracefully:

- ≥ 90×24: full avionics MFD
- ≥ 90×16: medium avionics MFD (including a 120×20 terminal)
- ≥ 56 wide: two-column odometer bars
- ≥ 24 wide: single-column compact metrics
- < 24×6: "enlarge terminal" message

### "Context scope looks distorted"

Terminal cells are typically twice as tall as wide. The scope compensates for
that cell geometry, but terminals with unusual font aspect ratios may still
stretch the circular display.

## Performance

### Large number of session files

The discovery scan recursively walks `~/.codex/sessions`. With thousands of files:

- Discovery backs off to 120s when idle
- Only changed files (mtime/len) are re-parsed
- Streaming `BufRead` parser avoids loading entire files into memory

To reduce scan time, set `codex_import.sessions_dir` to a smaller subset.

### Network requests

Claude OAuth fetch has a 5-second timeout. If the API is slow, the UI continues rendering cached data while the fetch runs in the background.

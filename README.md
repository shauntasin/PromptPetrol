# PromptPetrol

PromptPetrol is a Rust terminal dashboard for monitoring Claude and Codex
subscription usage. It renders five-hour and weekly utilization as an avionics
multi-function display, with provider instrument bays flanking the active Codex
session's context scope.

## Features

- Live Claude five-hour and weekly utilization from the Claude OAuth usage API.
- Local Codex five-hour and weekly limits from Codex CLI session logs.
- Active Codex context-window usage from the latest session.
- Full and medium avionics MFD layouts, including a dedicated 120×20 display mode.
- Compact odometer bars on narrower terminals.
- Four palettes across full and compact display modes: Murphy, Paper, Arctic,
  and Solarized Light. The latter three are light themes.
- A 2 Hz render loop separated from the configurable data-refresh interval.
- Incremental in-memory caching of unchanged Codex session files.
- Recursive Codex session discovery with idle backoff from 10 to 120 seconds.
- Config reload with `r` and an in-app keyboard reference with `?`.

PromptPetrol currently displays subscription utilization and context usage. It
does not calculate API spend, import provider billing records, or persist usage
history.

## Requirements

- A terminal with color and Unicode support.
- Rust stable with edition 2024 support.
- Codex CLI session logs, Claude Code credentials, or both.
- macOS Keychain access for automatic Claude token discovery. On other systems,
  set the Claude OAuth token explicitly in the config file.

## Run

```bash
cargo run
```

Command-line options:

```text
PromptPetrol - monitor AI subscription usage in your terminal

Usage: promptpetrol [OPTIONS]

Options:
  --config-file <PATH>               Use a specific configuration file
  --refresh-interval-seconds <SECS>  Refresh data at this interval [default: 10]
  -h, --help                         Print help
  -V, --version                      Print version
```

The refresh interval accepts positive fractional values such as `0.5`. Avoid
very short intervals when Claude fetching is enabled because every data refresh
can make a network request.

## Controls

- `q`: quit
- `r`: reload the selected config file and refresh data
- `?`: toggle keyboard help
- `t`: cycle through color themes for the current run

## Configuration

The default config path is `~/.config/promptpetrol/config.json`. PromptPetrol
creates missing parent directories and restricts the config file to mode `0600`
on Unix because it may contain an OAuth token.

```json
{
  "theme": "murphy",
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

- `theme`: selects `murphy`, `paper`, `arctic`, or `solarized-light`. Murphy is
  the default when the field is omitted. Pressing `t` temporarily overrides the
  configured theme until PromptPetrol exits.
- `codex_import.enabled`: enables local Codex session ingestion.
- `codex_import.sessions_dir`: overrides the default `~/.codex/sessions` path.
- `claude_import.enabled`: enables Claude credential discovery and API fetching.
- `claude_oauth_token`: optional Claude OAuth token. Leaving it `null` enables
  macOS Keychain discovery.

Unknown config fields are ignored for compatibility with older config files. If
a config reload fails, PromptPetrol keeps the last valid configuration and marks
the dashboard title with `CONFIG ERROR`.

## Claude Usage

The Claude token is resolved in this order:

1. A non-empty `claude_oauth_token` in the selected config file.
2. The macOS Keychain entry named `Claude Code-credentials`.

PromptPetrol sends the token as a Bearer credential only to
`https://api.anthropic.com/api/oauth/usage`. The endpoint returns utilization
percentages and reset times, which are shown directly in the Claude gauges.

## Codex Usage

PromptPetrol recursively reads `.jsonl` files from `~/.codex/sessions`, or from
the configured override. It uses the latest `token_count` events for rate limits
and context-window data.

Context-window fill is calculated as:

```text
input_tokens - cached_input_tokens + output_tokens
```

Only files whose modification time or length changed are reparsed. File
discovery starts at a 10-second cadence and gradually backs off to 120 seconds
after idle cycles; changing the configured sessions directory invalidates the
old cache immediately.

## Troubleshooting

- No Codex gauges: confirm the sessions directory exists and contains readable
  `.jsonl` files with `token_count` events.
- Stale Codex gauges: press `r`; discovery may be in its idle backoff window.
- No Claude gauges: set `claude_oauth_token` or confirm the Claude Code Keychain
  item is available to the current user.
- `Auth failed (401/403)`: refresh the Claude OAuth credential.
- `CONFIG ERROR` in the title: validate the selected JSON config file, then
  press `r`.
- Broken layout: enlarge the terminal. Below 24 columns or 6 rows, PromptPetrol
  displays an enlargement prompt.

## Development

Planning artifacts:

- [Product roadmap](docs/ROADMAP.md)
- [Research-backed scaling plan](docs/SCALING_PLAN.md)
- [Dependency-ordered implementation checklist](TODO.md)

Run the same checks as CI:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Ignored tests are visual dumps and a local large-tree performance probe:

```bash
cargo test -- --ignored
```

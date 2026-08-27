# Product Roadmap

PromptPetrol remains a local-first resource cockpit. The detailed technical
sequence, evidence, budgets, and exit gates live in
[SCALING_PLAN.md](SCALING_PLAN.md); executable work is tracked in
[`TODO.md`](../TODO.md).

## Stage 1 - Trustworthy Local Monitor

- Freshness and source-health annunciation
- Complete Claude HTTP failure tests and retry policy
- Verified terminal restoration
- Live-provider validation without credential exposure

**Release outcome:** every displayed number clearly communicates whether it is
current, stale, unavailable, rate limited, or unauthorized.

## Stage 2 - Scalable Local Ingest

- Provider-neutral snapshots
- Persistent cache-owning source workers
- O(1) render-time snapshot access
- Append-only Codex JSONL parsing
- Watcher acceleration with periodic reconciliation
- Reproducible 10,000-file performance suite

**Release outcome:** the UI remains responsive and incremental refresh cost is
driven by new data rather than total session history.

## Stage 3 - Operational Intelligence

- Durable normalized history
- Burn-rate and runway forecasting
- Trends and Data Link MFD pages
- Threshold and source-health alerts with hysteresis

**Release outcome:** PromptPetrol explains trajectory and risk, not only the
current percentage.

## Stage 4 - Provider And Platform Expansion

- Third-provider adapter based on the proven normalized contract
- Cross-platform secure credential storage
- Reproducible and signed releases
- Optional multi-profile support

**Release outcome:** new sources and platforms do not require changes to the
core display model.

## Deferred Until Explicitly Approved

- Hosted service or central team dashboard
- Remote collection daemon
- Plugin ABI
- Billing and cost estimation
- Web dashboard

These features introduce materially different security, tenancy, migration,
and support obligations. They are not prerequisites for a strong local tool.

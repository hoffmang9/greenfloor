# ADR 0012: Manager CLI Rust orchestration cutover

## Status

Accepted (2026-05-29)

## Context

GreenFloor historically implemented manager `build-and-post-offer` in Python
(`greenfloor/runtime/offer_orchestration.py`) with Dexie/Splash adapters, SQLite
persistence, and bootstrap/create/publish orchestration.

The Rust engine now owns the same vertical slice for the **manager CLI path**:

- `greenfloor-engine build-and-post-offer`
- Python `greenfloor-manager build-and-post-offer` delegates via subprocess only

The **daemon** (`greenfloord`) still uses the Python orchestration stack until
the daemon runtime migrates to Rust.

## Decision

1. **Manager CLI = Rust only.** Python must not parse program/markets YAML or
   resolve venue URLs for `build-and-post-offer`. Optional CLI overrides are
   passed through to Rust; Rust resolves canonical settings.

2. **Rust owns manager config schema** for this path (`config/program.rs`,
   `config/markets.rs`) and SQLite persistence schema (`storage/sqlite.rs`).

3. **Rust owns manager file logging** for this path (`manager/logging.rs`), writing
   to `{home_dir}/logs/debug.log` with `app.log_level` from program config.

4. **Python orchestration is legacy for daemon only.** Do not extend
   `offer_orchestration.py` for new manager CLI behavior. Bug fixes that affect
   both paths should land in Rust first, then daemon cutover.

## Cutover milestones

| Milestone                         | Owner               | Delete when done                        |
| --------------------------------- | ------------------- | --------------------------------------- |
| Manager CLI subprocess delegation | Python CLI          | N/A (keep thin wrapper)                 |
| Rust build/post + sqlite persist  | `greenfloor-engine` | —                                       |
| Daemon cycle offer post           | Python today        | `offer_orchestration.py` manager path   |
| Daemon sqlite / config            | Python today        | duplicate schema in `storage/sqlite.py` |

**Target:** When `greenfloord` runs offer build/post through `greenfloor-engine`
in-process or subprocess, delete Python `execute_build_and_post_offer` and shrink
`offer_orchestration.py` to tests-only fixtures or remove entirely.

## Consequences

- Manager operators get Rust logging/persistence parity without Python preflight.
- Two orchestration implementations remain until daemon migration; new features
  for manager CLI land in Rust only.
- CI must build/install `greenfloor-engine` for manager CLI tests and e2e.

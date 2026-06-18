# ADR 0012: Manager CLI Rust orchestration cutover

## Status

Superseded by [0013-rust-cli-daemon-native-cutover.md](0013-rust-cli-daemon-native-cutover.md) (2026-06-17)

## Context

GreenFloor historically implemented manager `build-and-post-offer` in Python
(`greenfloor/runtime/offer_orchestration.py`) with Dexie/Splash adapters, SQLite
persistence, and bootstrap/create/publish orchestration.

The Rust engine now owns the same vertical slice for the **manager CLI path**:

- `greenfloor-engine build-and-post-offer`
- `greenfloor-manager build-and-post-offer` (native binary)

The daemon runs natively via `greenfloord` → `greenfloor-engine daemon`.

## Decision

1. **Manager CLI = Rust only.** Python must not parse program/markets YAML or
   resolve venue URLs for `build-and-post-offer`. Optional CLI overrides are
   passed through to Rust; Rust resolves canonical settings.

2. **Rust owns manager config schema** for this path (`config/program.rs`,
   `config/markets.rs`) and SQLite persistence schema (`storage/sqlite.rs`).

3. **Rust owns manager file logging** for this path (`manager/logging.rs`), writing
   to `{home_dir}/logs/debug.log` with `app.log_level` from program config.

4. **Python orchestration removed.** `greenfloor/cli/` and `greenfloor/daemon/`
   deleted; see ADR 0013.

## Cutover milestones

| Milestone                         | Owner               | Status |
| --------------------------------- | ------------------- | ------ |
| Native `greenfloor-manager` CLI   | `greenfloor-engine` | Done   |
| Native `greenfloord` daemon       | `greenfloor-engine` | Done   |
| Rust build/post + sqlite persist  | `greenfloor-engine` | Done   |
| Delete Python CLI/daemon packages | repo                | Done   |

## Consequences

- Manager operators use Rust logging/persistence parity without Python preflight.
- Single orchestration implementation in Rust for manager and daemon paths.
- CI builds/installs all native binaries for manager/daemon tests and e2e.

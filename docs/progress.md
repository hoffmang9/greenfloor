# Progress Log

Current architecture and recent milestones. Older migration history lives in git
history and superseded ADRs (`0006`–`0012`).

## Current architecture (2026-06)

**Operators (production):** native Rust binaries only.

| Binary               | Role                                                       |
| -------------------- | ---------------------------------------------------------- |
| `greenfloor-manager` | Config, keys, cats, coin ops, build/post, offers lifecycle |
| `greenfloord`        | Market cycle daemon (`--once` or loop)                     |
| `greenfloor-engine`  | Low-level engine CLI (vault debug, legacy subcommands)     |

Implementation lives in `greenfloor-engine/src/`:

- `manager_cli/` — manager command dispatch and JSON output
- `daemon/` — cycle loop, market phases, websocket tx signals
- `offer/operator/` — shared build/post + signer denomination (manager + daemon)
- `offer/lifecycle/` — reconcile, cancel, status (manager + daemon)
- `coin_ops/` + `daemon/coin_ops_execution/` — coin-op policy and execution
- `cycle/` — deterministic strategy, cancel policy, parallel dispatch
- `storage/` — SQLite schema and persistence

**Python package (`greenfloor/`):** config parsing, HTTP adapters, and PyO3 bridges
(`greenfloor/core/*_bridge.py`) for parity tests and scripts. Not used by operator
binaries. See ADR 0013 for PyO3 scope.

**Deleted:** `greenfloor/cli/`, `greenfloor/daemon/`, Python offer/coin-op orchestration
runtime modules.

## Recent milestones

### 2026-06-17 — Rust-native CLI/daemon cutover (ADR 0013)

- Native `greenfloor-manager` and `greenfloord`; Python console scripts removed.
- All V1 manager commands in `manager_cli/`; daemon cycle fully in Rust.
- Migration catch-up: `docs/rust-migration-ledger.md`.

### 2026-06-17 — Module boundary cleanup

- Removed `manager/` shim; shared orchestration in `offer/operator` and `offer/lifecycle`.
- Signer denomination decomposed under `offer/operator/signer_denomination/`.
- Unified manager JSON output (`emit_json` / `emit_serialized`); coin-op errors return
  payloads to command boundary instead of emitting from mid-stack helpers.
- Trimmed crate-root re-exports in `lib.rs`; operator binaries import domain modules
  directly.

## Active live testing

- **Mainnet canary:** `eco1812022_sell_wusdbc` (`ECO.181.2022:wUSDC.b`). See runbook
  §2 mainnet cutover checklist.
- **Testnet11 proof pair:** `TDBX:txch` (historical G1–G3 closure; CI via
  `live-testnet-e2e.yml`).

## References

- Operator procedures: `docs/runbook.md`
- V1 scope and open items: `docs/plan.md`
- Breaking changes: `docs/rust-migration-ledger.md`
- Architecture decisions: `docs/decisions/` (start with ADR 0013)

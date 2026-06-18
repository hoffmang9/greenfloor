# 0010 - Rust Engine Crate Naming

## Status

Accepted

Updated 2026-06-17: PyO3 extension removed; scripts call `greenfloor-engine` CLI subcommands.

## Context

The Rust implementation was introduced as a signer for vault KMS paths. The crate
now owns deterministic daemon policy: cycle orchestration, offer reconciliation,
and coin-op planning. The old "signer" name no longer describes the scope.

## Decision

**Completed:**

- Rust source directory: `greenfloor-engine/`
- Cargo package and CLI binary: `greenfloor-engine`
- Rust library target: `greenfloor_engine` (crate name used by operator binaries)
- Policy grouped by domain module (`cycle/`, `coin_ops/`, `offer/`, `vault/`, `coinset_cli/`)

**Removed (2026-06-17):**

- `greenfloor-engine-pyo3/` and the `greenfloor_engine` Python extension module
- `greenfloor.core.engine_bridge` and all Python policy bridges
- In-process Python↔Rust FFI for operator or script paths

**Script Coinset IO:** `greenfloor-engine coinset {push-tx,fee-estimate,conservative-fee-estimate}`
via `greenfloor.adapters.coinset` (subprocess, JSON stdout).

**Integration tests:** `greenfloor-engine daemon-once --request-json <file> --json`
(requires `GREENFLOOR_DAEMON_TEST_CONTROLS=1` when `test_controls` is non-default).

## Naming map

| Layer           | Name                 |
| --------------- | -------------------- |
| Cargo crate     | `greenfloor-engine`  |
| Rust lib target | `greenfloor_engine`  |
| Manager binary  | `greenfloor-manager` |
| Daemon binary   | `greenfloord`        |

## Consequences

- Operator and script paths use native binaries only; no Python extension install.
- ADR 0006/0007 boundaries unchanged; this ADR clarifies naming after PyO3 removal.

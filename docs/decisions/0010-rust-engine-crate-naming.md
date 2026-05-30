# 0010 - Rust Engine Crate Naming

## Status

Accepted

Updated 2026-05-29: the Rust source directories, Cargo package, CLI binary,
Rust library target, and PyO3 module now use engine naming.

## Context

The Rust implementation was introduced as a signer for vault KMS paths. The crate
now also owns deterministic daemon policy: cycle orchestration, offer
reconciliation, and coin-op planning. The old "signer" name no longer describes
the scope.

## Decision

**Completed during this migration phase:**

- Rename source paths to `greenfloor-engine/` and `greenfloor-engine-pyo3/`.
- Rename the Cargo package and CLI binary to `greenfloor-engine`, and the Rust
  library target and PyO3 module to `greenfloor_engine`.
- `greenfloor.core.engine_bridge.import_engine()` remains the canonical Python
  bridge API for in-process Rust policy; `import_signer` remains a migration alias.
- `engine_bridge` imports `greenfloor_engine`; use `engine_rebuild_hint(module=...)`
  and `require_engine_method()` for operator rebuild text and stale-symbol errors.
- Group Rust policy by domain module (`cycle/`, `coin_ops/`, `offer/`, `vault/`) inside the crate.

**Remaining compatibility:**

- Retain `greenfloor.core.engine_bridge.import_signer` as a Python migration alias
  until legacy call sites disappear.

## Naming map

| Layer                | Current                         | Target                     |
| -------------------- | ------------------------------- | -------------------------- |
| Cargo crate          | `greenfloor-engine`             | done                       |
| PyO3 module          | `greenfloor_engine`             | done                       |
| Python bridge        | `engine_bridge.import_engine()` | unchanged                  |
| Vault/sign path docs | "Rust signer"                   | "Rust engine (vault path)" |

## Consequences

- New Python policy surfaces use `engine_bridge`, not ad-hoc `importlib` copies.
- Adapter IO paths (`rust_signer`, `coinset`, `native_offer`, etc.) import
  the engine through `engine_bridge.import_engine()`.
- Legacy module shims (`greenfloor.core.fee_budget`, `inventory`, `coin_ops_policy`) were
  removed once call sites imported `greenfloor.core.coin_ops` only; coin-op policy now lives
  in `_bridge.py` with `CoinOpsEngineProtocol` typing the PyO3 surface.
- ADR 0006/0007 remain valid; this ADR clarifies naming without changing boundaries.

# 0010 - Rust Kernel Crate Naming

## Status

Accepted

## Context

The Cargo crate `greenfloor-signer` and PyO3 module `greenfloor_signer` were introduced for
vault KMS signing. The crate now also owns deterministic daemon policy: cycle orchestration,
offer reconciliation, and coin-op planning. The name "signer" no longer describes the scope.

## Decision

**Near term (this migration phase):**

- Keep crate path `greenfloor-signer/` and PyO3 module `greenfloor_signer` to avoid breaking
  CI, maturin wheels, and operator installs mid-migration.
- Introduce Python `greenfloor.core.kernel_bridge.import_kernel()` as the canonical import for
  in-process Rust policy; `import_signer` remains a migration alias.
- Group Rust policy by domain module (`cycle/`, `coin_ops/`, `offer/`, `vault/`) inside the
  crate regardless of the legacy crate name.

**End state (post migration):**

- Rename Cargo crate to `greenfloor-kernel`.
- Rename PyO3 module to `greenfloor_kernel`.
- Retain `greenfloor_signer` as a deprecated re-export shim for one release if needed.

## Naming map

| Layer | Current | Target |
|-------|---------|--------|
| Cargo crate | `greenfloor-signer` | `greenfloor-kernel` |
| PyO3 module | `greenfloor_signer` | `greenfloor_kernel` |
| Python bridge | `kernel_bridge.import_kernel()` | unchanged |
| Vault/sign path docs | "Rust signer" | "Rust kernel (vault path)" |

## Consequences

- New Python policy surfaces use `kernel_bridge`, not ad-hoc `importlib` copies.
- Adapter IO paths (`rust_signer`, `coinset`, `bls_signing`, `native_offer`, etc.) import
  the kernel through `kernel_bridge.import_kernel()`.
- Legacy module shims (`greenfloor.core.fee_budget`, `inventory`, `coin_ops_policy`) were
  removed once call sites imported `greenfloor.core.coin_ops` only; coin-op policy now lives
  in `_bridge.py` with `CoinOpsKernelProtocol` typing the PyO3 surface.
- ADR 0006/0007 remain valid; this ADR clarifies naming without changing boundaries.
- Full rename is deferred until Python daemon/CLI glue migration is closer to complete.

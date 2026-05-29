# 0006 - Rust Signer Canonical Path

## Status

Accepted

## Decision

`greenfloor-engine` is the canonical signing implementation for vault KMS paths.
Python signing in `greenfloor/signing.py` remains during migration but is **legacy**;
new vault spend, mixed-split, and offer behavior lands in Rust first.

Migration order:

1. **Now:** `greenfloor-engine` CLI for operator/debug flows and CI parity tests.
2. **Next:** Python daemon/CLI invoke `greenfloor-engine` (or a PyO3 wrapper) instead of
   duplicating spend logic in `greenfloor/signing.py`.
3. **End state:** Remove duplicated Python vault spend paths; Python keeps orchestration,
   config, and adapters only.

Cloud Wallet GraphQL offer posting (`greenfloor/adapters/cloud_wallet.py`) may remain
until local Rust offer paths reach production parity for each market flow.

## Rationale

- Vault MIPS + KMS fast-forward signing is easier to test and keep correct in Rust with
  `chia-wallet-sdk` path deps.
- Python `signing.py` duplicated puzzle construction increases drift risk (presplit nonce,
  member hashes, mode-23 relay).
- ADR 0002 consolidated Python layers; this ADR supersedes its "no alternate stacks"
  guidance for the **implementation** layer while preserving adapter boundaries.

## Consequences

- Feature work for vault CAT spends and offers targets `greenfloor-engine/` first.
- Python parity tests validate cross-language hash/spend contracts during migration.
- ADR 0002 canonical Python APIs become thin wrappers until removed.
- Coinset IO in Rust (`greenfloor-engine/src/coinset/`) is allowed for signer paths;
  Python adapters remain for daemon orchestration until cutover.
- Vault CAT coin selection for offers and mixed splits goes through
  `OfferCoinsetBackend` (live coinset adapter + simulator test backend).
- `--split-input-coins` selects the presplit-new input mode, but when selected CAT
  inputs already equal `--offer-amount` exactly the signer still uses the direct
  offer execution path (`execution_mode: direct`); no vault split spend is emitted.

## Supersedes

Partially supersedes ADR 0002 "no alternate signing stacks" — see migration plan above.

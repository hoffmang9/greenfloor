# ADR 0007: Rust signer and Coinset IO

## Status

Accepted (2026-05); operator scope finalized by
[0013-rust-cli-daemon-native-cutover.md](0013-rust-cli-daemon-native-cutover.md) (2026-06-17).

> **Note:** This ADR was originally titled “Rust Signer PyO3 Boundary.” PyO3 and Python
> policy bridges were removed in 2026-06-17; the filename was updated to match current
> architecture.

## Current state

- **Operators:** vault KMS signing, offer creation, offer cancel/reclaim spends, bootstrap
  mixed-splits, and asset resolution run in-process in `greenfloor-engine`.
- **Scripts:** Coinset push/fee via `greenfloor-engine coinset …` CLI subcommands.
- **On-chain IO:** direct Coinset HTTP API via Rust (`greenfloor-engine/src/coinset/`).
- Cloud Wallet GraphQL is removed from offer/bootstrap/asset paths.

## Original decision (2026-05)

Vault KMS signing and offer construction are implemented once in `greenfloor-engine`.
PyO3 briefly provided in-process access for Python orchestration and parity tests.
**Removed 2026-06-17** (ADR 0013).

## Rationale

- Single canonical Rust implementation reduces drift (presplit nonce, MIPS signing).
- Direct Coinset RPC (including asset lookup) replaces GraphQL custody snapshot reads.

## Deferred

- Rust simulator atomic-take roundtrip for CAT:CAT requested legs remains
  sell-CAT/request-XCH only; buy-side and cat-cat fixtures validate offer build shape.
- On-chain presplit-existing cancel: covered by simulator e2e (ADR 0015); live mainnet
  cancel proof tracked separately in operator runbooks.

## Consequences

- CI builds `greenfloor-engine` binaries only; operators install Rust binaries.
- Golden offer fixtures export from Rust simulator tests.

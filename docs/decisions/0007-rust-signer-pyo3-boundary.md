# 0007 - Rust Signer PyO3 Boundary

## Status

Accepted; **operator scope updated** by [0013-rust-cli-daemon-native-cutover.md](0013-rust-cli-daemon-native-cutover.md) (2026-06-17)

## Current state

- **Operators:** vault KMS signing, offer creation, bootstrap mixed-splits, and asset
  resolution run in-process in `greenfloor-engine` (no PyO3 on the install path).
- **Python library / tests:** the same Rust logic is reachable via `greenfloor_engine`
  (PyO3) through `greenfloor.core.engine_bridge` and `*_bridge.py` modules.
- **On-chain IO:** Coinset MSP via Rust (`greenfloor-engine/src/coinset/`).
- Cloud Wallet GraphQL is removed from offer/bootstrap/asset paths.

## Original decision (2026-05)

Vault KMS signing and offer construction are implemented once in `greenfloor-engine`.
PyO3 provided in-process access for Python orchestration and parity tests.

## Rationale

- Single canonical Rust implementation reduces drift (presplit nonce, MIPS signing).
- Coinset MSP `get_singleton_info` replaces GraphQL custody snapshot reads.

## Deferred

- Rust simulator atomic-take roundtrip for CAT:CAT requested legs remains
  sell-CAT/request-XCH only; buy-side and cat-cat fixtures validate offer build shape.

## Consequences

- CI builds `greenfloor-engine-pyo3` for parity tests; operators install Rust binaries only.
- Golden offer fixtures export from Rust simulator tests.

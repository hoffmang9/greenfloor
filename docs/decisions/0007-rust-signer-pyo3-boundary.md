# 0007 - Rust Signer PyO3 Boundary

## Status

Accepted

## Decision

Vault KMS signing, offer creation, bootstrap mixed-splits, and asset ID resolution run
through the in-process `greenfloor_signer` PyO3 extension backed by `greenfloor-signer`.

Configuration and vault member metadata come from `program.yaml` (`signer:` + `vault:`).
On-chain IO uses `api-msp.coinset.org` via the Rust MSP coinset client.

Cloud Wallet GraphQL is removed from offer/bootstrap/asset paths. Python retains
orchestration (manager, daemon, Dexie/Splash publish, ladder planning).

## Rationale

- Single canonical Rust implementation reduces drift (presplit nonce, MIPS signing).
- PyO3 avoids subprocess hops (ADR 0002 alignment).
- Coinset MSP `get_singleton_info` replaces GraphQL custody snapshot reads.
- Operator installs reset with updated yaml; no dual-path feature flags.

## Deferred (documented)

- Full deletion of Cloud Wallet GraphQL adapter methods (`create_offer`, `split_coins`) remains
  until all operator installs migrate off Cloud Wallet offer paths.
- Rust simulator scenarios for buy-side and CAT-request fixtures are covered at the Python
  orchestration layer; dedicated `buy_side.json` / `cat_cat.json` exports are deferred.
- PyO3 ↔ CLI JSON parity test deferred until the signer CLI stabilizes on the same request schema.

## Consequences

- CI builds `greenfloor-signer-pyo3` wheel alongside `greenfloor-native`.
- Golden offer fixtures export from Rust simulator tests.
- Python tests validate wiring and `validate_offer`; Rust tests validate spend semantics.

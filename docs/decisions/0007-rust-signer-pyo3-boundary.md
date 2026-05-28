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

- Rust simulator atomic-take roundtrip for CAT:CAT requested legs remains sell-CAT/request-XCH only;
  buy-side and cat-cat fixtures validate offer build + `create_offer_request` shape.
- Operator `greenfloor-signer create-offer` CLI parity with live Coinset is covered by PyO3 in
  production paths; dedicated subprocess JSON parity tests are not required while PyO3 is canonical.

## Consequences

- CI builds `greenfloor-signer-pyo3` wheel alongside `greenfloor-native`.
- Golden offer fixtures export from Rust simulator tests.
- Python tests validate wiring and `validate_offer`; Rust tests validate spend semantics.

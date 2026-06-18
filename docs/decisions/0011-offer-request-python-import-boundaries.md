# 0011 - Offer Request Python Import Boundaries

## Status

**Superseded** by [0013-rust-cli-daemon-native-cutover.md](0013-rust-cli-daemon-native-cutover.md) (2026-06-17).
Python policy bridges and PyO3 FFI were deleted; this ADR is historical.

## Context (2026-05)

Offer-request leg math lives in `greenfloor-engine`. Python bridges needed stable import
paths without growing `policy_bridge.py` into a flat FFI catalog.

## Original decision

### Canonical Rust surface

- `greenfloor-engine/src/offer/request.rs` — leg math, side/asset normalization,
  `compute_signer_offer_leg_amounts`.

### Deleted Python modules (2026-06-17)

The following were removed with `greenfloor/core/` and `greenfloor-engine-pyo3/`:

- `greenfloor.core.offer_request_bridge`
- `greenfloor.core.policy_bridge`
- `greenfloor.core.offer_bootstrap_bridge`
- `greenfloor.core.signer_offer_request`
- `greenfloor.core.offer_action`
- `greenfloor.adapters.offer_action`

## Current rule

Operator build/post and daemon offer execution are Rust-native (`offer/operator/`,
`offer/lifecycle/`). Do not reintroduce Python policy bridges or PyO3 symbols.

## Consequences

- Offer creation uses signer vault KMS via `greenfloor-engine` only.
- Scripts that need Coinset mutations call `greenfloor-engine coinset …` subcommands.

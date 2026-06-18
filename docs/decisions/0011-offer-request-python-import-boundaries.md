# 0011 - Offer Request Python Import Boundaries

## Status

Accepted (2026-05-28); **operator orchestration superseded** by ADR 0013. Bridge import
rules still apply to the remaining Python library and tests.

## Context

Offer-request leg math lives in `greenfloor-engine`. Python bridges need stable import
paths without growing `policy_bridge.py` into a flat FFI catalog.

## Decision

### Canonical Rust surface

- `greenfloor-engine/src/offer/request.rs` — leg math, side/asset normalization,
  `compute_signer_offer_leg_amounts`.

### Python modules (import from here)

| Module                                   | Use for                                       |
| ---------------------------------------- | --------------------------------------------- |
| `greenfloor.core.offer_request_bridge`   | Offer-request leg math and side normalization |
| `greenfloor.core.policy_bridge`          | Pricing/publish/retry engine wrappers         |
| `greenfloor.core.offer_bootstrap_bridge` | Bootstrap planner and phase wrappers          |
| `greenfloor.core.signer_offer_request`   | `SignerCreateOfferRequest` shaping            |
| `greenfloor.core.offer_action`           | Typed action request/result (pure)            |
| `greenfloor.adapters.offer_action`       | Engine IO (`build_signer_offer_for_action`)   |

### Rules

- All offer creation uses signer vault KMS via `core/offer_action` +
  `adapters/offer_action`. Do not reintroduce legacy BLS or Cloud Wallet builders.
- New PyO3 symbols go in domain modules (`offer_request_py.rs`, etc.) and surface
  through the matching `*_bridge.py`, not ad hoc `policy_bridge` growth.

## Consequences

- Operator build/post is Rust-native (`offer/operator/`); these bridges serve tests,
  scripts, and any remaining Python callers only.

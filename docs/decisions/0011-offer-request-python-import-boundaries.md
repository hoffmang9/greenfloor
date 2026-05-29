# 0011 - Offer Request Python Import Boundaries

## Status

Accepted (2026-05-28)

## Context

Offer-request leg math moved into `greenfloor-signer` with PyO3 bindings. Python needs stable
import paths for runtime, daemon dispatch, vault request construction, and BLS offer building
without growing `policy_bridge.py` into a flat FFI catalog.

## Decision

### Canonical Rust surface

- `greenfloor-signer/src/offer/request.rs` — leg math, validation, `normalize_offer_side`,
  `normalize_offer_asset_id`, `signer_split_asset_id`, `compute_signer_offer_leg_amounts`.

### Python modules (import from here, not `policy_bridge`)

| Module | Use for |
|--------|---------|
| `greenfloor.core.offer_request_bridge` | Direct kernel access to offer-request symbols (internal bridge). |
| `greenfloor.core.offer_policy` | **Stable runtime/daemon/BLS imports** — re-exports leg math + Dexie/publish helpers. |
| `greenfloor.core.signer_offer_request` | `SignerCreateOfferRequest`, `SignerOfferLegAmounts`, `build_signer_create_offer_request`. |

### `policy_bridge.py` role

- Owns **pricing/publish/retry** kernel wrappers and re-exports offer-request symbols for
  backward compatibility during migration.
- **New code** should import offer-request helpers from `offer_policy` or
  `signer_offer_request`, not add new `policy_bridge` call sites.

### Normalized offer side caching

- `prepare_offer_build_context()` normalizes `action_side` once; `OfferBuildContext.action_side`
  is always `"buy"` or `"sell"`.
- `PlannedAction.side` from the cycle kernel is already `"buy"` or `"sell"`; dispatch uses
  `planned_action_side()` (no kernel round-trip) instead of re-normalizing per hop.
- `normalize_offer_side()` in `offer_request_bridge` uses a fast path for common inputs and
  calls the kernel only for non-standard values; parity tests lock equivalence.

## Consequences

- Next offer-migration PRs add symbols to `offer_request_py.rs` + `offer_request_bridge.py`, not
  `offer_build_py.rs` / `policy_bridge.py` bodies.
- Removing `core/offer_side.py` was intentional; do not reintroduce a pass-through module.

# 0011 - Offer Request Python Import Boundaries

## Status

Accepted (2026-05-28); updated 2026-05-29 (signer-only stack)

## Context

Offer-request leg math moved into `greenfloor-engine` with PyO3 bindings. Python needs stable import paths for runtime, daemon dispatch, and vault request construction without growing `policy_bridge.py` into a flat FFI catalog.

## Decision

### Canonical Rust surface

- `greenfloor-engine/src/offer/request.rs` — leg math, validation, `normalize_offer_side`,
  `normalize_offer_asset_id`, `signer_split_asset_id`, `compute_signer_offer_leg_amounts`.

### Python modules (import from here, not duplicated facades)

| Module                                   | Use for                                                                                        |
| ---------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `greenfloor.core.offer_request_bridge`   | Direct engine access to offer-request symbols (leg math, side normalization).                  |
| `greenfloor.core.policy_bridge`          | Pricing/publish/retry engine wrappers (`verify_offer_for_dexie`, retry sleeps, publish gates). |
| `greenfloor.core.offer_bootstrap_bridge` | Bootstrap DTOs, planner, and phase engine wrappers.                                            |
| `greenfloor.core.signer_offer_request`   | Low-level `SignerCreateOfferRequest` / `signer_create_offer_request_from_fields`.              |
| `greenfloor.core.offer_action`           | Canonical offer create — typed action request/result, pure shaping, create-phase mapping.      |
| `greenfloor.adapters.offer_action`       | Engine IO only (`build_signer_offer_for_action`).                                              |

### Offer-action create path

- **All market-action offer creation** uses `core/offer_action` + `adapters/offer_action` (signer vault KMS).
- Do not add call sites to legacy BLS or Cloud Wallet offer builders.

### `policy_bridge.py` role

- Owns **pricing/publish/retry** engine wrappers.
- **New code** should import offer-request helpers from `offer_request_bridge` and publish/retry helpers from `policy_bridge`.

### Normalized offer side caching

- `prepare_offer_build_context()` normalizes `action_side` once; `OfferBuildContext.action_side`
  is always `"buy"` or `"sell"`.
- `PlannedAction.side` from the cycle engine is already `"buy"` or `"sell"`; dispatch uses
  `planned_action_side()` (no engine round-trip) instead of re-normalizing per hop.
- `normalize_offer_side()` in `offer_request_bridge` uses a fast path for common inputs and
  calls the engine only for non-standard values; parity tests lock equivalence.

## Consequences

- Next offer-migration PRs add symbols to `offer_request_py.rs` + `offer_request_bridge.py`, not
  `offer_build_py.rs` / `policy_bridge.py` bodies.
- Bootstrap planner symbols use `offer_bootstrap_bridge.py` and `offer_bootstrap_py.rs` (not
  `offer_build_py.rs`). Bridges call `engine_bridge.bootstrap_engine()` (`BootstrapEngineProtocol`).
- Removed: `core/offer_policy.py`, `core/retry_policy.py`, `core/offer_bootstrap_policy.py`, local BLS offer modules.
- Removing `core/offer_side.py` was intentional; do not reintroduce a pass-through module.

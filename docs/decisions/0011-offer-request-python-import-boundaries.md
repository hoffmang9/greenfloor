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

| Module                                    | Use for                                                                                                                                           |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `greenfloor.core.offer_request_bridge`    | Direct kernel access to offer-request symbols (internal bridge).                                                                                  |
| `greenfloor.core.offer_bootstrap_bridge`  | **Stable runtime imports** — bootstrap DTOs, planner, and phase kernel wrappers.                                                                  |
| `greenfloor.core.offer_bootstrap_policy`  | Backward-compatible re-export of `offer_bootstrap_bridge` (no logic).                                                                             |
| `greenfloor.core.offer_policy`            | **Stable runtime/daemon/BLS imports** — re-exports leg math + Dexie/publish helpers.                                                              |
| `greenfloor.core.signer_offer_request`    | **Deprecated for offer create** — low-level `SignerCreateOfferRequest` / `build_signer_create_offer_request` (KMS plan-dict + parity tests only). |
| `greenfloor.core.offer_action`            | **Canonical offer create** — typed action request/result, pure shaping, create-phase outcome mapping.                                             |
| `greenfloor.runtime.offer_action_request` | Build action requests from `OfferBuildContext`.                                                                                                   |
| `greenfloor.runtime.offer_action_build`   | Local/signer runtime orchestration (asset resolution + BLS create).                                                                               |
| `greenfloor.adapters.offer_action`        | Kernel IO only (`build_*_offer_for_action`).                                                                                                      |

### Offer-action create path (2026-05)

- **New market-action offer creation** must use `core/offer_action` + `adapters/offer_action`
  (signer) or `runtime/offer_action_build` (local BLS). Do not add call sites to
  `build_signer_create_offer_request` / `rust_signer.build_vault_cat_offer` for that flow.
- Local BLS resolves ticker symbols via `resolve_action_assets_for_build_context` before kernel
  dispatch when ids are not already canonical.

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
- Bootstrap planner symbols use `offer_bootstrap_bridge.py` and `offer_bootstrap_py.rs` (not
  `offer_build_py.rs`). Bridges call `kernel_bridge.bootstrap_kernel()` (`BootstrapKernelProtocol`).
  Kernel API: `plan_bootstrap_mixed_outputs(ladder_entries=...)` returns `BootstrapPlanOutcome`
  (`ready` / `needs_split` / `cannot_fund` / `invalid_ladder` / `invalid_coins`).
- Rust layout: `greenfloor-signer/src/offer/bootstrap/planner.rs` (deficit planner),
  `offer/bootstrap/phase.rs` (early/executed phase snapshots). PyO3 marshalling:
  `greenfloor-signer-pyo3/src/py_utils/bootstrap_marshal.rs`.
- Runtime orchestration lives in `greenfloor/runtime/offer_bootstrap.py`
  (`BootstrapRuntimeDeps`, `BootstrapPreflight`, `BootstrapSplitExecution`). Phase DTOs live in
  `greenfloor.offer_bootstrap`; early/executed phase mapping is Rust via `offer_bootstrap_bridge.py`.
- **Fee eligibility** (non-zero split fee guard) is intentionally Python-only in
  `run_bootstrap_preflight`; do not move fee I/O into the Rust phase table.
- Planner input DTO: `PlannerLadderRow` (config uses `MarketLadderEntry`).
- Removing `core/offer_side.py` was intentional; do not reintroduce a pass-through module.

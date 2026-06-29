# ADR 0014: Offer publish module decomposition and bootstrap gate collapse

## Status

Accepted (2026-06-21; bootstrap layout refined in [0017](0017-offer-submodule-decompositions.md))

## Context

`greenfloor-engine/src/offer/publish/mod.rs` had grown into the largest non-test Rust module
outside the submodule. It mixed Dexie posting, publish-side asset expectations, Dexie row
visibility checks, and bootstrap offer-creation gating. Bootstrap gating also duplicated
status/reason mapping across string fields, snapshot helpers, and operator result types.

## Decision

1. **Split publish into venue-focused submodules.** `offer/publish/` now holds Dexie posting
   (`dexie/`) and publish-side asset helpers (`assets/expectations.rs`, `assets/visibility.rs`).
   Bootstrap offer-creation **gating policy** lives in `offer/bootstrap/gate.rs`; typed phase
   snapshots and status mapping live in `offer/bootstrap/phase/` (see ADR 0017).

2. **Collapse bootstrap block API.** Offer build/post checks
   `BootstrapPhaseResult::offer_creation_block_error()`. Early-phase snapshots use
   `bootstrap_phase_snapshot_block_error(&snapshot)` from `offer/bootstrap/gate.rs` (re-exported
   as `offer::bootstrap::bootstrap_phase_snapshot_block_error`). Typed `BootstrapPhaseStatus`
   is stored on both `BootstrapPhaseSnapshot` and `BootstrapPhaseResult`; JSON serialization
   still emits the legacy `status` string field via `BootstrapPhaseStatus::as_str()`.

3. **Keep publish-side asset normalization internal.** `OfferSideAssets` and
   `offer_side_assets_for_side` are `pub(crate)` — only `assets/expectations.rs` needs them.

4. **Flat publish entrypoint.** `publish_offer` takes flat arguments instead of a
   `PublishOfferParams` struct.

## Removed `offer::` re-exports (library consumers only)

These symbols are no longer exported from `greenfloor_engine::offer`:

| Removed symbol                                  | Replacement                                                    |
| ----------------------------------------------- | -------------------------------------------------------------- |
| `bootstrap_block_error`                         | `BootstrapPhaseResult::offer_creation_block_error()`           |
| `bootstrap_offer_gate`                          | internal `offer::bootstrap::gate` (not public)                 |
| `BootstrapOfferGate`                            | internal `offer::bootstrap::gate` (not public)                 |
| `bootstrap_blocks_offer`                        | `BootstrapPhaseResult::offer_creation_block_error().is_some()` |
| `ExpectedPublishAssetFieldsRef`                 | `ExpectedPublishAssetFields` (owned)                           |
| `dexie_offer_asset_expectation_error`           | `offer::publish::assets::visibility` (crate-internal)          |
| `OfferSideAssets`, `offer_side_assets_for_side` | crate-internal via `offer::request`                            |

Operator CLI JSON shapes are unchanged: bootstrap results still serialize `status` as
`"failed"`, `"executed"`, or `"skipped"`.

## Consequences

- External Rust crates depending on removed `offer::` re-exports must update imports or call
  operator JSON paths instead.
- Bootstrap gating has a single typed path: construct snapshots in `phase/`, interpret blocking
  in `gate/` (`BootstrapPhaseStatus` → `BootstrapOfferGate` → block error string).
- Publish module stays focused on venue posting and Dexie asset visibility.

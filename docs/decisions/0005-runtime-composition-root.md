# 0005 - Runtime Composition Root

## Status

Accepted (updated 2026-05-26 — see also ADR 0008)

## Decision

Adopt `greenfloor.runtime.offer_execution` as the single named runtime composition root for offer build/post orchestration and runtime fee-resolution wiring.

- Daemon and manager runtime flows import orchestration helpers from `greenfloor.runtime.offer_execution` (or focused submodules in tests).
- Runtime-level Coinset fee/preflight helpers consumed by manager/signing route through the same composition root (`coinset_runtime.py`).
- Implementation is split across focused modules under `greenfloor/runtime/` (orchestration, publish, build context, post request dispatch, signer backend, `cloud_wallet/*`, local BLS). See ADR 0008 for the module map.
- The former `greenfloor/runtime/cloud_wallet_offer_runtime.py` monolith was removed; do not reintroduce a single-file offer runtime.

## Rationale

- Clarifies architecture boundaries by giving side-effect orchestration one explicit runtime surface.
- Reduces top-level import sprawl and makes trust/IO review easier.
- Enables gradual extraction over time (pure transforms → `core/`, HTTP/SDK interactions → `adapters/`) without breaking existing functionality.

## Consequences

- New runtime wiring should import via `greenfloor.runtime.offer_execution`.
- CLI `build-and-post-offer` lives in `greenfloor/cli/offer_build_post.py`; manager re-exports only.
- Daemon managed offer post uses `OfferPostRequest` + `managed_offer_execution_backend()` — no CLI import, no injectable `build_and_post_fn`.
- Follow-up work belongs in the existing runtime modules (ADR 0008), not new top-level runtime entry files.

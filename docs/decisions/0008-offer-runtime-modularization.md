# 0008 - Offer Runtime Modularization

## Status

Accepted (2026-05-26); updated 2026-05-29 (signer-only stack)

## Decision

Replace the monolithic `greenfloor/runtime/cloud_wallet_offer_runtime.py` with focused runtime modules and a shared orchestration layer. CLI and daemon both dispatch through `OfferPostRequest`; pricing and expiry come from `OfferBuildContext`.

### Composition root

- `greenfloor.runtime.offer_execution` — re-exports the public runtime surface for in-repo callers.

### Shared orchestration

- `greenfloor/runtime/offer_orchestration.py` — bootstrap → create → verify → publish loop (`build_and_post_offer`, `execute_build_and_post_offer`).
- `greenfloor/runtime/offer_publish.py` — venue-neutral verify/post helpers, quote/expiry resolution, Dexie visibility checks.
- `greenfloor/runtime/offer_build_context.py` — `OfferBuildContext`, `prepare_offer_build_context()`, keyring/program-path helpers.

### Dispatch and backends

- `greenfloor/runtime/offer_post_request.py` — `OfferPostRequest` (CLI + daemon routing), `parse_managed_offer_post_result()`.
- `greenfloor/runtime/offer_runtime.py` — vault KMS / Rust signer backend (`build_and_post_offer_signer`).
- `greenfloor/runtime/offer_reconciliation.py` — thin CLI wrapper over Rust `reconcile_offers_batch` (canonical orchestration in `greenfloor-engine/src/daemon/reconcile_{phase,batch,persist}.rs`).

### CLI entry

- `greenfloor/cli/offer_build_post.py` — `build_and_post_offer_cli()`; `greenfloor/cli/manager.py` re-exports it as `_build_and_post_offer`.

### Routing gates (`greenfloor/config/models.py`)

- `require_signer_offer_path()` / `require_coin_ops_signer_path()` — raise when KMS + vault are not configured.
- `signer_offer_path_configured()` — boolean gate for daemon skip reasons.

### Backend contracts

- Signer entry points take `build_ctx: OfferBuildContext` only (program/market derived from context).
- `build_and_post_offer()` takes `build_ctx` and derives quote price / action side internally.

## Rationale

- The former ~2,259-line module mixed polling, bootstrap, asset resolution, CLI dispatch, and publish logic.
- Duplicated dispatch in CLI and daemon caused drift (signer prep, pricing source, injectable callbacks).
- `OfferBuildContext` gives one canonical model for quote price, expiry, side, and keyring paths.

## Consequences

- Deleted: `greenfloor/runtime/cloud_wallet_offer_runtime.py`, Cloud Wallet backend modules, local BLS offer paths.
- Removed: daemon `build_and_post_fn` injection; managed post uses `OfferPostRequest.run_managed()` directly.
- Daemon must not import CLI for offer build/post; it uses runtime modules and `adapters/offer_action`.
- Tests split from `tests/test_manager_post_offer.py` into focused modules (`test_offer_cli_dispatch.py`, `test_offer_post_request.py`, etc.).
- New offer-runtime wiring should extend existing modules rather than reintroducing a monolith.

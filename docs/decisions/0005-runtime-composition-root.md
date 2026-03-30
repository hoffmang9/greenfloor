# 0005 - Runtime Composition Root

## Status

Accepted

## Decision

Adopt `greenfloor.runtime.offer_execution` as the single named runtime composition root for offer-execution orchestration and runtime fee-resolution wiring.

- Daemon and manager runtime flows import orchestration/runtime helpers from `greenfloor.runtime.offer_execution`.
- Runtime-level Coinset fee/preflight helpers consumed by manager/signing are routed through the same composition-root module.
- Existing modules (`greenfloor/cloud_wallet_offer_runtime.py`, `greenfloor/coinset_runtime.py`) remain implementation units for now, while dependency entrypoints converge at the runtime root.

## Rationale

- Clarifies architecture boundaries by giving side-effect orchestration one explicit runtime surface.
- Reduces top-level import sprawl and makes trust/IO review easier.
- Enables gradual extraction over time (pure transforms -> `core/`, HTTP/SDK interactions -> `adapters/`) without breaking existing functionality.

## Consequences

- New runtime wiring should import via `greenfloor.runtime.offer_execution`.
- Future runtime orchestration additions should be attached through this composition root rather than introducing new top-level runtime entry modules.
- Follow-up cleanup can deprecate legacy direct imports once callers are fully migrated.

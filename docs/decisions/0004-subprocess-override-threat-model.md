# 0004 - Subprocess Override Threat Model

## Status

Superseded

## Decision

This note previously documented operator subprocess escape hatches.

Those override hooks were removed from active code paths:

- `GREENFLOOR_OFFER_BUILDER_CMD`
- `GREENFLOOR_WALLET_EXECUTOR_CMD`

Current policy is direct in-process execution for offer build and wallet execution paths.

## Rationale

- The project intentionally reduced default subprocess boundaries (`0002`) to improve reliability and testability.
- At the time this note was introduced, overrides existed for migration/troubleshooting.
- The project now prefers strict in-process execution to reduce complexity and trust-surface area.

## Consequences

- Runtime execution paths are simpler and no longer include these subprocess escape hatches.
- Any future reintroduction of subprocess override hooks requires a new architecture decision.

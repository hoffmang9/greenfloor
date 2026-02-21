# AGENTS.md

This file defines implementation conventions for coding agents and contributors.

## Core Expectations

- Work at a senior-developer standard: prefer explicit tradeoffs and maintainable design.
- If uncertain about behavior or requirements, ask for clarification before coding.
- Treat `chia-wallet-sdk` (repo submodule) as the default library for blockchain syncing, spend-bundle signing, and offer-file generation in GreenFloor.
- Treat offer files as `offer1...` Bech32m strings produced/consumed via `chia-wallet-sdk` offer encode/decode contracts.
- Treat offer cancellation as exceptional: only for stable-vs-unstable pairs, only on strong unstable-side price moves, and not as routine lifecycle management.
- Ensure all posted offers have expiry; stable-vs-unstable pair offers should use shorter expiries.
- Keep the architecture boundaries strict:
  - `greenfloor/core`: deterministic policy logic only (no IO).
  - `greenfloor/config`: parse/validate configuration.
  - `greenfloor/* adapters`: side effects (network, filesystem, wallet, notifications).

## Required Pre-Implementation Review

- Read all repo markdown docs in root/docs before major implementation changes.
- Review legacy behavior in `old/*.py` before changing market-making semantics.

## Testing and Quality Gates

- Python minimum version: 3.11.
- PR-required deterministic test harness must complete under 10 minutes wall clock (prefer under 5).
- Required PR checks:
  - `ruff check`
  - `ruff format --check`
  - `pyright`
  - deterministic tests (`pytest`)

## Notifications (V1 Scope)

- Only low-inventory alerts are in scope for v1.
- Alert payload must include ticker, remaining amount, and receive address.

## Progress and Decisions

- Update `docs/progress.md` for major milestones.
- Add a decision note in `docs/decisions/` for non-trivial architecture/policy changes.

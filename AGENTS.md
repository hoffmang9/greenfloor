# AGENTS.md

Implementation policy for coding agents and contributors.

## Rule Priority

When rules conflict, apply this order: correctness > safety > architecture > style > convenience.

Severity tags:

- `[MUST]`: required.
- `[SHOULD]`: expected unless a documented reason exists.
- `[CONTEXT]`: current scope or intent (can change as the project evolves).

## Core Policy

- `[MUST]` Work at a senior-developer standard: explicit tradeoffs and maintainable design.
- `[MUST]` If behavior or requirements are unclear, ask before coding.
- `[MUST]` Use `chia-wallet-sdk` (repo submodule) for blockchain sync, signing, and offer encode/decode.
- `[MUST]` Treat offers as `offer1...` Bech32m strings from `chia-wallet-sdk` contracts.
- `[MUST]` Fix the primary path; do not add fallback execution paths to hide correctness gaps.
- `[SHOULD]` Temporary sdk symbol-rename shims are allowed only during explicit migrations and must be removed once the pinned baseline stabilizes.
- `[MUST]` Network symbol discipline: mainnet uses `xch`, testnet11 uses `txch` in examples, defaults, runbooks, workflows, and operator commands.
- `[MUST]` CAT denomination discipline: 1000 mojos of a CAT is exactly 1 unit of that CAT in examples, operator output, runbooks, tests, and code comments.
- `[SHOULD]` When debugging, prefer the existing log pipeline: set the host log level to `DEBUG` in `program.yaml` and use the service logs instead of adding ad hoc debug code or one-off debug files.
- `[SHOULD]` Offer cancellation is exceptional (stable-vs-unstable only, and only on strong unstable-side moves).
- `[MUST]` All posted offers must include expiry; stable-vs-unstable pairs should use shorter expiries.

## Architecture Boundaries

- `[MUST]` `greenfloor/core`: deterministic policy only (no IO).
- `[MUST]` `greenfloor/config`: parse/validate config, resolve paths, resolve quote assets.
- `[MUST]` `greenfloor/* adapters`: side effects only (network, filesystem, wallet, notifications).
- `[MUST]` `greenfloor/signing.py`: unified signing entry point (coin discovery, spend-bundle construction, broadcast).
- `[MUST]` `greenfloor/cli/manager.py`: operator CLI commands.
- `[MUST]` `greenfloor/cli/offer_builder_sdk.py`: offer text construction.
- `[MUST]` Reuse canonical utilities: `greenfloor/hex_utils.py`, `greenfloor/logging_setup.py`, `greenfloor/config/io.py`.
- `[MUST]` Import direction: daemon never imports CLI; CLI never imports daemon. Shared logic belongs in shared modules.

## Design Constraints

- `[MUST]` Prefer direct function calls within the package; do not spawn subprocesses for same-env Python calls unless isolation/security is documented in `docs/decisions/`.
- `[MUST]` Signing/execution path is 2 layers max: adapter -> `greenfloor/signing.py`.
- `[MUST]` Avoid unnecessary indirection layers (`executor`, `worker`, `engine`, etc.).
- `[MUST]` Keep one distinct responsibility per file; merge pass-through modules into functions.
- `[MUST]` Eliminate duplicated logic blocks (>10 lines) by extracting shared helpers.
- `[MUST]` Use allowlists for state checks; never rely on negated blocklists.
- `[MUST]` For similar polling loops, match existing interval/accumulator style; warning cadence must be additive (`next_warning += warning_interval`).
- `[SHOULD]` Keep functions under ~150 logic lines (excluding docstrings/blank lines).
- `[SHOULD]` Minimize module-level mutable state; pass mutable state through objects/dataclasses.

## Current Scope

- `[CONTEXT]` Critical path: configure market -> build real offer -> post to venue -> verify on-chain.
- `[CONTEXT]` Defer new CLI commands, metrics, and observability work until live-target proof exists for the active market.
- `[CONTEXT]` v1 notifications cover only low-inventory alerts and must include ticker, remaining amount, and receive address.

## Before You Commit

- `[MUST]` Python version is 3.11+.
- `[MUST]` Use venv binaries for Python tooling (for example `.venv/bin/python -m pytest`).
- `[MUST]` Run `pre-commit run --all-files`.
- `[MUST]` Every conditional dispatch gate has deterministic tests for each branch.
- `[MUST]` Every `while True` + `time.sleep` loop has deterministic timeout/warning tests (mock `time.sleep` and `time.monotonic`).
- `[MUST]` Extract repeated test setup into a named helper when it appears in more than two tests.
- `[SHOULD]` Deterministic test harness runtime stays under 10 minutes wall clock (target under 5).

## Review and Decisions

- `[MUST]` Before major behavior changes, read markdown docs in `docs/` and review git history or archived notes for legacy behavior when relevant.
- `[MUST]` Record major milestones in `docs/progress.md`.
- `[MUST]` Add a decision note in `docs/decisions/` for non-trivial architecture or policy changes.

# AGENTS.md

Implementation policy for coding agents and contributors.

## Rule Priority

When rules conflict, apply this order: correctness > safety > architecture > style > convenience.

When "minimize diff scope" conflicts with commit scope or pre-commit (see **Commit scope** below),
**commit scope wins**.

Severity tags:

- `[MUST]`: required.
- `[SHOULD]`: expected unless a documented reason exists.
- `[CONTEXT]`: current scope or intent (can change as the project evolves).

## Core Policy

- `[MUST]` Work at a senior-developer standard: explicit tradeoffs and maintainable design.
- `[MUST]` If behavior or requirements are unclear, ask before coding.
- `[MUST]` Use `chia-wallet-sdk` (repo submodule) for blockchain sync, signing, and offer validation contracts.
- `[MUST]` Treat offers as `offer1...` Bech32m strings. Offer encode/decode uses `greenfloor_engine` via `greenfloor.offer_decode`.
- `[MUST]` Fix the primary path; do not add fallback execution paths to hide correctness gaps.
- `[SHOULD]` Temporary sdk symbol-rename shims are allowed only during explicit migrations and must be removed once the pinned baseline stabilizes.
- `[MUST]` Network symbol discipline: mainnet uses `xch`, testnet11 uses `txch` in examples, defaults, runbooks, workflows, and operator commands.
- `[MUST]` CAT denomination discipline: 1000 mojos of a CAT is exactly 1 unit of that CAT in examples, operator output, runbooks, tests, and code comments.
- `[SHOULD]` When debugging, prefer the existing log pipeline: set the host log level to `DEBUG` in `program.yaml` and use the service logs instead of adding ad hoc debug code or one-off debug files.
- `[MUST]` Do not create new markdown, scripts, or notes outside `docs/`, `.cursor/`, or paths the user requested. Prefer editing existing project files.
- `[SHOULD]` Offer cancellation is exceptional (stable-vs-unstable only, and only on strong unstable-side moves).
- `[MUST]` All posted offers must include expiry; stable-vs-unstable pairs should use shorter expiries.

## Architecture Boundaries

- `[MUST]` `greenfloor/core`: deterministic policy only (no IO).
- `[MUST]` `greenfloor/core/coin_ops/`: coin-op deterministic policy (plan, fee budget, inventory, min-amount guard) shared by CLI and daemon.
- `[MUST]` `greenfloor/config`: parse/validate config, resolve paths, resolve quote assets.
- `[MUST]` `greenfloor/* adapters`: side effects only (network, filesystem, wallet, notifications).
- `[MUST]` Signing/execution path is adapter -> canonical Rust engine (`greenfloor-engine` crate / `greenfloor_engine` PyO3).
- `[MUST]` `greenfloor-engine/`: canonical Rust engine crate; new vault spend/offer logic lands here first.
- `[MUST]` `greenfloor/cli/manager.py`: operator CLI router (argparse + dispatch).
- `[MUST]` `greenfloor/cli/coin_ops_list.py`, `coin_ops_split.py`, `coin_ops_combine.py`: coin list/split/combine CLI commands (`coin_ops.py` re-exports).
- `[MUST]` `greenfloor/cli/cats.py`: local CAT catalog CLI commands.
- `[MUST]` `greenfloor/cli/offers_lifecycle.py`: offer reconcile/status/cancel CLI wrappers (core logic in `runtime/offer_reconciliation.py`).
- `[MUST]` `greenfloor/cli/manager_setup.py`: config validate, doctor, bootstrap-home, set-log-level.
- `[MUST]` `greenfloor/cli/keys_onboard.py`: keys-onboard CLI command.
- `[MUST]` `greenfloor/cli/offer_build_post.py`: manager `build-and-post-offer` command implementation.
- `[MUST]` `greenfloor/runtime/coin_ops/runtime.py`: shared coin-op orchestration for CLI and daemon.
- `[MUST]` `greenfloor/runtime/coin_ops/steps.py`: split/combine iteration step bodies.
- `[MUST]` `greenfloor/runtime/offers_cancel.py`: venue offer cancel selection and Dexie execution.
- `[MUST]` `greenfloor/runtime/offer_reconciliation.py`: thin CLI wrapper over Rust `reconcile_offers_batch` (canonical reconcile orchestration in `greenfloor-engine`).
- `[MUST]` Offer build/post uses `adapters/offer_action.build_signer_offer_for_action` and `runtime/offer_post_request.OfferPostRequest` (signer/KMS only).
- `[MUST]` `greenfloor/runtime/offer_execution.py`: composition root for offer build/post runtime; import orchestration helpers here (see ADR 0005, ADR 0008).
- `[MUST]` Reuse canonical utilities: `greenfloor/hex_utils.py`, `greenfloor/logging_setup.py`, `greenfloor/config/io.py`.
- `[MUST]` Import direction: daemon never imports CLI; CLI never imports daemon. Shared logic belongs in shared modules.

## Design Constraints

- `[MUST]` Prefer direct function calls within the package; do not spawn subprocesses for same-env Python calls unless isolation/security is documented in `docs/decisions/`.
- `[MUST]` Signing/execution path is adapter -> canonical Rust engine (`greenfloor-engine` crate, `greenfloor_engine` PyO3 module).
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

## Commit scope

- `[MUST]` After `pre-commit run --all-files` passes, include every tracked change
  required for CI — not only hand-edited feature lines.
- `[MUST]` Hook and formatter output is never "unrelated."
- `[MUST]` **Unrelated** means should-not-be-in-the-repo, not "outside the feature story."

**Canonical:** `.cursor/rules/git-workflow.mdc` → **Commit scope** (pre-commit bundle,
include/exclude lists).

## Review and Decisions

- `[MUST]` Before major behavior changes, read markdown docs in `docs/` and review git history or archived notes for legacy behavior when relevant.
- `[MUST]` Record major milestones in `docs/progress.md`.
- `[MUST]` Add a decision note in `docs/decisions/` for non-trivial architecture or policy changes.

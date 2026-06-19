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
- `[MUST]` Use `chia-wallet-sdk` (repo submodule) in `greenfloor-engine` for signing, puzzle/offer
  construction, and offer validation. Do not run a full node or in-process wallet sync.
- `[MUST]` Use Coinset.org HTTP and WebSocket APIs for blockchain access: coin lookup, mempool/tx
  signals, fee estimates, and tx submission (`greenfloor-engine/src/coinset/` and daemon websocket
  handlers). Scripts reach Coinset through `greenfloor-engine coinset …` subcommands.
- `[MUST]` Treat offers as `offer1...` Bech32m strings. Operator paths validate/decode in Rust.
- `[MUST]` Fix the primary path; do not add fallback execution paths to hide correctness gaps.
- `[MUST]` Network symbol discipline: mainnet uses `xch`, testnet11 uses `txch` in examples, defaults, runbooks, workflows, and operator commands.
- `[MUST]` CAT denomination discipline: 1000 mojos of a CAT is exactly 1 unit of that CAT in examples, operator output, runbooks, tests, and code comments.
- `[SHOULD]` When debugging, prefer the existing log pipeline: set the host log level to `DEBUG` in `program.yaml` and use the service logs instead of adding ad hoc debug code or one-off debug files.
- `[MUST]` Do not create new markdown, scripts, or notes outside `docs/`, `.cursor/`, or paths the user requested. Prefer editing existing project files.
- `[SHOULD]` Offer cancellation is exceptional (stable-vs-unstable only, and only on strong unstable-side moves).
- `[MUST]` All posted offers must include expiry; stable-vs-unstable pairs should use shorter expiries.

## Architecture Boundaries

**Rust operators (production)**

- `[MUST]` `greenfloor-engine/` owns operator policy and execution: `config/`, `offer/`, `vault/`,
  `coin_ops/`, `daemon/`, `storage/`, `coinset/`.
- `[MUST]` Operator binaries are native Rust: `greenfloor-manager`, `greenfloord`, `greenfloor-engine`
  (`manager_cli/`, `daemon/`). No PyO3 or Python orchestration entrypoints.
- `[MUST]` Shared offer orchestration: `offer/operator/` and `offer/lifecycle/` (manager + daemon).
- `[MUST]` Coin-op policy and execution: `coin_ops/` and `daemon/coin_ops_execution/`.
- `[MUST]` Offer build/post uses `offer::operator::build_and_post_offer`
  (`greenfloor-manager build-and-post-offer` and daemon managed post).
- `[MUST]` Offer-payload ID extraction and conservative-fee parsing are Rust-only
  (`offer::dexie_payload`, `coinset::get_conservative_fee_estimate`).
- `[MUST]` Import direction: daemon never imports CLI; CLI never imports daemon. Shared logic lives in
  shared modules. Operator binaries import CLI modules directly (`manager_cli`, `daemon::cli`).

**Python (scripts and test harnesses only)**

- `[MUST]` `scripts/` and `scripts/greenfloor_scripts/` are adapters only — no operator policy or
  orchestration. Do not reintroduce PyO3, Python policy bridges, or `greenfloor/cli/` /
  `greenfloor/daemon/` runtime modules.
- `[MUST]` Config field reads: `scripts/greenfloor_scripts/config_subprocess.py` → `greenfloor-manager program-fields`,
  `markets-fields`, `cats-fields`, `materialize-minimal-program`, `config-validate`. Operator YAML
  policy lives in `greenfloor-engine/src/config/`; scripts must not walk operator YAML for policy
  fields.
- `[MUST]` Script adapters live under `scripts/greenfloor_scripts/` (subprocess bridges to native binaries).
  Coinset IO uses `greenfloor-engine coinset post` and `coinset push-tx`. Hex helpers use
  `greenfloor-engine hex` via `hex_subprocess`. Config field reads use `greenfloor-manager program-fields`,
  `markets-fields`, and `cats-fields`. KMS public-key fetch uses `greenfloor-engine kms-public-key-compressed-hex`.
- `[CONTEXT]` Pytest covers script subprocess adapters (`tests/test_script_subprocess.py`);
  Rust subprocess integration tests cover CLI contracts; operator policy parity is
  `cargo test --manifest-path greenfloor-engine/Cargo.toml` in CI (ADR 0013).

## Design Constraints

- `[MUST]` Prefer direct function calls within Rust operator code and within a Python module;
  do not spawn subprocesses for same-env Rust calls or Python-to-Python calls unless
  isolation/security is documented in `docs/decisions/`.
- `[MUST]` `scripts/greenfloor_scripts/` subprocess bridges to native binaries (`greenfloor-engine`,
  `greenfloor-manager`) are the canonical script IO path — not a violation of the rule above.
- `[MUST]` Avoid unnecessary indirection layers (`executor`, `worker`, `engine`, etc.).
- `[MUST]` Keep one distinct responsibility per file; merge pass-through modules into functions.
- `[MUST]` Eliminate duplicated logic blocks (>10 lines) by extracting shared helpers.
- `[MUST]` Use allowlists for state checks; never rely on negated blocklists.
- `[MUST]` For similar polling loops, match existing interval/accumulator style; warning cadence
  must be additive (`next_warning += warning_interval`). See `docs/plan.md` → **Delivery
  constraints** for deterministic test expectations on wait/timeout paths.
- `[SHOULD]` Keep functions under ~150 logic lines (excluding docstrings/blank lines).
- `[SHOULD]` Minimize module-level mutable state; pass mutable state through objects/dataclasses.

## Current Scope

- `[CONTEXT]` V1 scope, architecture, open items, and delivery constraints: `docs/plan.md`.
- `[CONTEXT]` Recent milestones and live testing targets: `docs/progress.md`.

## Before You Commit

- `[MUST]` Python version is 3.11+.
- `[MUST]` Use venv binaries for Python tooling (for example `.venv/bin/python -m pytest`).
- `[MUST]` Run `pre-commit run --all-files`.
- `[MUST]` Operator/daemon test expectations: `docs/plan.md` → **Delivery constraints**
  (deterministic branch coverage, injected clocks for wait loops, harness runtime).

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

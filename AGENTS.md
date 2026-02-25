# AGENTS.md

This file defines implementation conventions for coding agents and contributors.

## Core Expectations

- Work at a senior-developer standard: prefer explicit tradeoffs and maintainable design.
- If uncertain about behavior or requirements, ask for clarification before coding.
- Treat `chia-wallet-sdk` (repo submodule) as the default library for blockchain syncing, spend-bundle signing, and offer-file generation in GreenFloor.
- Treat offer files as `offer1...` Bech32m strings produced/consumed via `chia-wallet-sdk` offer encode/decode contracts.
- Do not introduce fallback execution paths to mask primary-path correctness gaps; debug and fix the primary path directly.
- Narrow exception: temporary symbol-rename compatibility shims for upstream `chia-wallet-sdk` bindings are allowed only during explicit migration windows (for example `validate_offer` -> `verify_offer`, `from_input_spend_bundle_xch` -> `from_input_spend_bundle`). Treat these as short-lived and remove them once the pinned submodule baseline is stable.
- Network symbol discipline is mandatory: mainnet pairs use `xch`, testnet11 pairs use `txch`. Do not use `xch` in testnet11 pair examples, defaults, runbooks, workflows, or operator commands.
- Treat offer cancellation as exceptional: only for stable-vs-unstable pairs, only on strong unstable-side price moves, and not as routine lifecycle management.
- Ensure all posted offers have expiry; stable-vs-unstable pair offers should use shorter expiries.
- Keep the architecture boundaries strict:
  - `greenfloor/core`: deterministic policy logic only (no IO).
  - `greenfloor/config`: parse/validate configuration.
  - `greenfloor/* adapters`: side effects (network, filesystem, wallet, notifications).
  - `greenfloor/signing.py`: unified signing module (coin discovery, spend-bundle construction, broadcast).
  - `greenfloor/cli/manager.py`: operator CLI commands.
  - `greenfloor/cli/offer_builder_sdk.py`: offer text construction.

## Simplicity and Design Discipline

These rules exist because earlier implementation rounds introduced unnecessary complexity that had to be removed. Follow them strictly.

### Prefer direct function calls over subprocess chains

- Within the same Python package, always use direct function calls.
- Never spawn a subprocess to call another module in the same virtualenv unless there is an explicit isolation or security requirement documented in a decision note.
- One env-var escape hatch per boundary is acceptable for operator overrides (e.g. `GREENFLOOR_WALLET_EXECUTOR_CMD`, `GREENFLOOR_OFFER_BUILDER_CMD`). Do not add more than one override per call site.

### Do not build features ahead of the critical path

- The critical path is: configure market -> build real offer -> post to venue -> verify on-chain.
- Do not add CLI commands, metrics, observability, or operational tooling until the critical path works end-to-end on testnet.
- When in doubt, ask: "Does this help us post a real offer on testnet11?" If no, defer it.

### Keep file count proportional to distinct responsibilities

- Each source file should own a distinct, non-trivial responsibility.
- Never create a file whose only job is to validate inputs, marshal a payload, and forward to the next file. That is a function, not a module.
- If two files have the same structure (read JSON, validate, call next layer, return JSON), they should be one file with two functions.

### Limit indirection layers

- The signing/execution path must stay at 2 layers max: the adapter (WalletAdapter or offer_builder_sdk) calls `greenfloor/signing.py`. That's it.
- Do not introduce intermediate "executor", "passthrough", "worker", "signer", "builder", "engine" layers.
- If a new layer is genuinely needed, write a decision note in `docs/decisions/` explaining why.

### Manager CLI surface discipline

- The manager currently has 10 commands: `bootstrap-home`, `config-validate`, `doctor`, `keys-onboard`, `build-and-post-offer`, `offers-status`, `offers-reconcile`, `coins-list`, `coin-split`, `coin-combine`.
- Do not add new commands without explicit user request or a documented need tied to G1-G3 testnet proof.
- Each new command must have a test that exercises it end-to-end with deterministic fixtures.

### No verbatim duplication within a file

- If the same logic block (more than ~10 lines) appears more than once anywhere in the same file, extract it to a named helper function before committing. Do not land a PR with copy-pasted blocks.
- When implementing a second function whose core logic closely mirrors an existing one, read the existing function carefully before writing, extract the shared kernel first, and verify both callers use the extracted helper.

### Allowlist over blocklist for state checks

- When classifying entities by state (coin spendability, offer status, lock state), always write an explicit allowlist of valid states. Never negate a blocklist of known-bad states; unknown or transitional states are never implicitly safe.
- If a helper function already encodes the allowlist (`_is_spendable_coin`, etc.), all callers in the same module must use that helper. Do not reimplement the same check inline.

### Consistent accumulator patterns in polling loops

- When adding a wait/poll loop that resembles another loop already in the same file, read the existing loop first and match its interval/accumulator pattern exactly.
- Additive re-warning pattern required: `next_warning += warning_interval` (not `warning_interval += warning_interval`, which doubles geometrically). Verify by checking that the first warning fires at time T, the second at 2T, the third at 3T.

### Every dispatch gate must have tests for all branches

- When a function contains a conditional dispatch (e.g., `if cloud_wallet_configured and not dry_run`), each branch of the gate must have its own deterministic test: one that triggers the primary branch, one that falls through.
- Gates controlled by configuration state are especially error-prone; test both the "configured + active" and "not configured / dry_run" paths explicitly.

### Every wait loop must have a deterministic test

- Every function containing `while True` with `time.sleep` must have at least one deterministic test. The test must mock `time.sleep` (to eliminate wall-clock cost) and `time.monotonic` (to exercise timeout and warning paths). Tests that rely on real elapsed time are not acceptable.

### Extract test setup helpers at first duplication

- If the same setup block (config file writing, credential patching, env variable setting) appears in more than two test functions, extract it to a named helper function in the test file before adding the third instance.
- Named helpers are preferred over fixtures when the setup is a pure function of its arguments (no session/module scope needed). This keeps the extraction visible and avoids fixture magic.

## Required Pre-Implementation Review

- Read all repo markdown docs in root/docs before major implementation changes.
- Review legacy behavior in `old/*.py` before changing market-making semantics.

## Testing and Quality Gates

- Python minimum version: 3.11.
- For Python-related commands in this repo, use the project virtual environment binaries (for example `.venv/bin/python -m pytest`, `.venv/bin/python -m ruff`, `.venv/bin/python -m pyright`).
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

# Progress Log

## 2026-02-23 (simplification pass)

- Identified and fixed three simplification opportunities in the codebase:

**Dead `if dry_run` branch removed (`manager.py`):**
- `_build_and_post_offer` had two identical `return 0 if publish_failures == 0 else 2` statements
  guarded by `if dry_run:` / `else`. Both branches returned the same expression. Collapsed to one line.

**Duplicated subprocess-vs-direct offer-builder logic consolidated:**
- `manager.py` had `_build_offer_text_for_request` + `_build_offer_text_via_subprocess` (40 lines).
- `daemon/main.py` had `_build_offer_for_action` (65 lines) reimplementing the same subprocess/direct
  branching independently — any change to the subprocess contract required two edits.
- Moved the single canonical implementation to `offer_builder_sdk.build_offer_text(payload)`:
  checks `GREENFLOOR_OFFER_BUILDER_CMD`, spawns the subprocess if set, otherwise calls `build_offer()`
  directly. Raises `RuntimeError` on any failure.
- `_build_offer_text_for_request` in `manager.py` is now a one-line delegate to `build_offer_text`.
- `_build_offer_for_action` in `daemon/main.py` is now a try/except wrapper around `build_offer_text`.
- Removed `import shlex` and `import subprocess` from both `manager.py` and `daemon/main.py`.

**`config/editor.py` deleted (no production caller):**
- `greenfloor/config/editor.py` (128 lines) and `tests/test_config_editor.py` (96 lines) deleted.
- The module supported `config-history-list` and `config-history-revert` commands removed in the
  2026-02-21 simplification pass (CLI 21 → 7 commands). No production code imported it.
- Per AGENTS.md: "Do not build features ahead of the critical path."

- All 146 tests pass (3 skipped); full pre-commit suite passes: `ruff`, `ruff-format`, `prettier`,
  `yamllint`, `pyright`, `pytest`.

## 2026-02-23

- Fixed "Invalid Offer" rejection from Dexie (400 response) on branch `fix/proof-gating-and-doc-alignment`:
  - Root cause: maker's offered coin spends were missing the `ASSERT_PUZZLE_ANNOUNCEMENT` condition required to atomically link them to the settlement coin. The SDK's `Spends` auto-assertion mechanism only fires for `SpendKind::Settlement` coins; regular CAT/XCH offered coins are `SpendKind::Conditions` and receive no assertion automatically.
  - Fix: compute announcement ID as `sha256(settlement_puzzle_hash + tree_hash(notarized_payment))` using `clvm.alloc(notarized_payment).tree_hash()` and call `spends.add_required_condition(clvm.assert_puzzle_announcement(announcement_id))` before `spends.prepare(deltas)` in `greenfloor/signing.py`.
  - Removed dead `from_input_spend_bundle` legacy fallback from `_from_input_spend_bundle_xch` (the binding no longer exists in the pinned SDK; only `from_input_spend_bundle_xch` remains). Updated stale test that expected legacy-path preference.
  - Added `.tmp-artifacts/` to `.gitignore`.
  - All 151 tests pass; `ruff check`, `ruff format`, `pyright` clean.
  - Committed `3f53d72` (SSH-signed), pushed branch, manually dispatched `live-testnet-e2e.yml` (run `22321746028`).
  - Run `22321746028` (dry-run): completed successfully in 45s; produced valid offer `offer1qqr83wcuu2rykcmqvp` (1054 chars).
  - Run `22321996901` (live, `dry_run=false`): completed successfully in 54s; offer posted to Dexie testnet with zero failures — Dexie offer ID `2HU1urTFmbKRVbtVsNnShFE3D7BXSVQBoeCB7aGzGUXa`. G1 proof achieved on `fix/proof-gating-and-doc-alignment`.
  - Full pre-commit suite passes: `ruff`, `ruff-format`, `prettier`, `yamllint`, `pyright`, `pytest` (151 passed, 3 skipped).

## 2026-02-23 (earlier)

- Updated GreenFloor for the latest `chia-wallet-sdk` fork API rename set and submodule tip:
  - Bumped `chia-wallet-sdk` submodule to `hoffmang9/greenfloor-from-input-spend-bundle` (`5a87495f`).
  - Switched active call paths to prefer `validate_offer` and `from_input_spend_bundle_xch`, with temporary compatibility shims to legacy binding names during the SDK rename window.
  - Added deterministic unit coverage for both new-name and fallback paths in manager offer validation and signing offer construction.
- Completed CI-only proof-path delivery and validation for live testnet workflow:
  - Updated `README.md`, `docs/runbook.md`, `docs/plan.md`, and `docs/progress.md` to document the CI-only mnemonic execution path and proof artifact expectations.
  - Ran local quality gate successfully with virtualenv PATH pinned:
    - `PATH="/Users/hoffmang/src/greenfloor/.venv/bin:$PATH" .venv/bin/pre-commit run --all-files`
  - Created signed commit (`d09637e`) on branch `ci-live-testnet-proof-flow`, pushed branch, and opened PR `#9`.
  - Manually dispatched `.github/workflows/live-testnet-e2e.yml` with:
    - `network_profile=testnet11`
    - `pair=TDBX:txch`
    - `size_base_units=1`
    - `dry_run=false`
  - Confirmed workflow run success (`run_id=22288977570`) and artifact upload (`live-testnet-e2e-artifacts`).
- Added first-class offer verification through `chia-wallet-sdk` and enforced it before Dexie submission:
  - Added `verify_offer` binding in forked `chia-wallet-sdk` (`offer` decode + `Offer::from_spend_bundle` validation path), then bumped submodule pointer in GreenFloor.
  - Updated manager offer post flow to validate offer text with wallet-sdk before calling Dexie; invalid offers are now blocked pre-submit with explicit error reasons.
  - Added/updated deterministic tests for manager Dexie post flow to account for pre-submit validation behavior.
- Cleaned branch history for reviewer readability and revalidated CI health after force-push:
  - Rewrote branch into a logical two-commit stack focused on (1) signing/diagnostics path and (2) txch discipline + pre-Dexie verification.
  - Manually dispatched live workflow after rewrite and confirmed success (`run_id=22294247395`) on `ci-live-testnet-proof-flow`.
  - Previous successful verification run remains available (`run_id=22294007396`) for comparison.

## 2026-02-22

- Updated CI and live testnet workflows after operator validation:
  - CI `pytest` steps no longer use `-q`, so logs show full test output in Actions.
  - Ubuntu simulator harness CI step now enables extended tests with `GREENFLOOR_RUN_SDK_SIM_TESTS_FULL=1`.
  - Optional `live-testnet-e2e` workflow now uses `TESTNET_WALLET_MNEMONIC` for onboarding import path.
  - Fixed `live-testnet-e2e` manager command ordering so global flags are passed before `config-validate`.
  - Confirmed manual workflow-dispatch run succeeded after command-order fix.
  - Extended `live-testnet-e2e` into a CI-only manager proof path for G1/G3 evidence:
    - New workflow-dispatch inputs: `pair` (default `TDBX:txch`) and `size_base_units` (default `1`).
    - Workflow now runs manager golden-path commands in order: `doctor`, dry-run `build-and-post-offer`, live `build-and-post-offer` (when `dry_run=false`), `offers-status`, `offers-reconcile`.
    - Added artifact upload (`live-testnet-e2e-artifacts`) for command logs and daemon-cycle output.
    - Enabled a higher default derivation scan limit in workflow env (`GREENFLOOR_CHIA_KEYS_DERIVATION_SCAN_LIMIT=1000`) to reduce false negatives for funded CI wallet keys.

- Refreshed current repository quality-gate status after pre-commit workflow alignment:
  - Added `pre-commit` to dev dependencies and updated docs to use `pre-commit run --all-files` as the primary local gate command.
  - Expanded pre-commit local hooks to include both `pyright` and `pytest` (in addition to `ruff`, `ruff-format`, `prettier`, `yamllint`).
  - Latest full-suite test result: `133 passed, 2 skipped`.
  - Latest full pre-commit result: all hooks passed (`ruff`, `ruff-format`, `prettier`, `yamllint`, `pyright`, `pytest`).

- Replaced SDK RPC client usage in active signing/wallet coin paths with `CoinsetAdapter`:
  - `greenfloor/signing.py` coin discovery + CAT parent lineage reads + `push_tx` now call Coinset HTTP endpoints through `greenfloor/adapters/coinset.py`.
  - `greenfloor/adapters/wallet.py` XCH inventory reads now use `CoinsetAdapter` coin-record queries.
  - Removed legacy `GREENFLOOR_WALLET_SDK_COINSET_URL`; active Coinset override is `GREENFLOOR_COINSET_BASE_URL`.
  - Added deterministic adapter coverage in `tests/test_coinset_adapter.py` (network routing defaults, endpoint request/response handling) plus signing test coverage asserting testnet11 adapter routing.
- Verified quality gates after Coinset adapter migration:
  - `ruff check`
  - `pytest` full suite (`132 passed, 2 skipped`)

- Implemented in-process SDK offer-signing path for manager offer builds:
  - `greenfloor/signing.py` now supports `plan.op_type: "offer"` with direct spend construction/signing in-process.
  - Added CAT coin discovery path for selected receive-address puzzle hash + asset id, including parent-spend lineage reconstruction via Coinset `get_coin_record_by_name` and `get_puzzle_and_solution`.
  - Added mixed-asset offer action building (`Action.send`) with explicit requested-asset output and offered-asset change handling.
  - Preserved existing split/combine signing path for daemon coin-op execution.
- Updated manager offer builder contract:
  - `greenfloor/cli/offer_builder_sdk.py` now builds offer-plan payloads (`offer_asset_id`, `offer_amount`, `request_asset_id`, `request_amount`) instead of split-plan payloads.
  - Added quote/base multiplier and quote-price validation guards in coin-backed builder path.
- Added deterministic test coverage for offer-plan delegation and manager builder contract updates:
  - `tests/test_signing.py` adds offer-plan branch tests.
  - `tests/test_offer_builder_sdk.py` updated for offer-plan payload assertions.
- Ran live manager proof commands on `testnet11`:
  - `build-and-post-offer --pair CARBON22:txch --size-base-units 1 --network testnet11 --dry-run`
  - `build-and-post-offer --pair CARBON22:txch --size-base-units 1 --network testnet11`
  - Both currently fail with `signing_failed:no_unspent_offer_cat_coins`.
  - Verified configured receive address has zero XCH and zero CAT balances on `testnet11` across the seeded supported-asset list.
- Verified quality gates after implementation:
  - `ruff check`
  - `ruff format --check`
  - `pyright`
  - `pytest` (`122 passed, 2 skipped`)

## 2026-02-21

### Architecture simplification

Major codebase simplification targeting three areas of accidental complexity introduced by prior implementation rounds.

**Signing chain collapse (13 files -> 1 file):**

- Deleted 13 CLI modules that formed a deep subprocess chain (`wallet_executor` -> `chia_keys_executor` -> `chia_keys_signer_backend` -> `chia_keys_raw_engine_sign_impl_sdk_submit`, plus 9 legacy intermediaries: `passthrough`, `worker`, `signer`, `builder`, `bundle_signer`, `bundle_signer_raw`, `raw_engine`, `raw_engine_sign`, `raw_engine_sign_impl`).
- Created `greenfloor/signing.py` — a single module with direct function calls for coin discovery, coin selection, additions planning, spend-bundle construction + AGG_SIG signing, and broadcast.
- `WalletAdapter` now calls `signing.sign_and_broadcast()` directly instead of spawning subprocesses. External executor override preserved via `GREENFLOOR_WALLET_EXECUTOR_CMD`.

**Manager CLI stripped to 7 core commands (1,593 -> 897 lines):**

- Kept: `bootstrap-home`, `config-validate`, `doctor`, `keys-onboard`, `build-and-post-offer`, `offers-status`, `offers-reconcile`.
- Removed 14 commands that were premature before testnet proof: `keys-list`, `keys-test-sign`, `reload-config`, `register-coinset-webhook`, `set-low-watermark`, `consolidate`, `set-price-policy`, `coin-op-budget-report`, `metrics-export`, `list-supported-assets`, `config-history-list`, `config-history-revert`, `set-ladder-entry`, `set-bucket-count`.
- Removed commands are tracked in plan.md deferred backlog for re-addition after G1-G3.

**Offer builder subprocess boundary eliminated:**

- `_build_offer_text_for_request()` now calls `offer_builder_sdk.build_offer()` as a direct Python function. External override preserved via `GREENFLOOR_OFFER_BUILDER_CMD`.

**Test consolidation:**

- Deleted 22 test files (~2,000 lines) for removed code.
- Added `tests/test_signing.py` (15 tests) covering input validation, error propagation, additions planning, fingerprint parsing, and mock-based signing + broadcast flow.
- Updated `test_wallet_adapter.py` with new test for direct signing path (no subprocess).
- Updated `test_offer_builder_sdk.py` with tests for `build_offer()` public API and signing module delegation.
- Updated `test_manager_post_offer.py` with test for direct `_build_offer_text_for_request` call path.
- All 120 tests pass in 3.9s. All quality gates pass: `ruff check`, `ruff format`, `pyright`, `pytest`.

**Entrypoints cleaned up:**

- `pyproject.toml` reduced from 15 script entrypoints to 2 (`greenfloor-manager`, `greenfloord`).

**Governance updates:**

- Updated `AGENTS.md` with new "Simplicity and Design Discipline" section: rules for preferring direct calls over subprocess chains, not building features ahead of the critical path, keeping file count proportional to responsibilities, limiting indirection layers, and manager CLI surface discipline.
- Updated `docs/plan.md` to reflect simplified signing architecture, corrected TODO state, added explicit deferred backlog section, and added emphasis that G1-G3 are the only priorities.
- Updated `README.md` to reflect current 7-command CLI surface and simplified env-var contract.

## 2026-02-20

- Added explicit v1 plan doc (`docs/plan.md`) and clarified that `chia-wallet-sdk` submodule is the default syncing/signing library.
- Updated `AGENTS.md` and `README.md` to align implementation guidance with the `chia-wallet-sdk` submodule baseline.
- Extended plan + guidance docs to explicitly include `chia-wallet-sdk` as the default offer-file generation path.
- Updated signer-chain tests to use Chia-style fingerprint key IDs and valid Chia bech32m addresses from `chia-wallet-sdk` address test vectors.
- Consolidated the active default signing pipeline to 4 layers (`wallet_executor` -> `chia_keys_executor` -> `chia_keys_signer_backend` -> `sdk_submit`) and recorded the architecture decision in `docs/decisions/0002-signing-pipeline-consolidation.md`.
- Implemented `DexieAdapter.post_offer()` and `DexieAdapter.cancel_offer()` and updated tests to use a real Offer-file fixture (`offer1...`) generated from `chia-wallet-sdk` offer test data.
- Clarified offer-management policy in plan/docs: cancellation is rare and only for stable-vs-unstable pairs on strong unstable-leg price moves; all offers expire, and stable-vs-unstable pair offers use shorter expiries.
- Added minimal Coincodex price adapter (`greenfloor/adapters/price.py`) with TTL cache and stale fallback, and wired daemon strategy evaluation to consume cached XCH price snapshots each cycle.
- Updated `evaluate_market()` to require a valid XCH price snapshot before planning XCH offers (USDC strategy remains price-independent), with added deterministic tests for price-gated planning behavior.
- Implemented manager `bootstrap-home` command for real deployment preflight: creates `~/.greenfloor` runtime layout (`config`, `db`, `state`, `logs`), seeds config templates into home config, rewrites `app.home_dir`, and initializes SQLite state DB.
- Updated `README.md` quickstart to make `greenfloor-manager bootstrap-home` the explicit first deployment prerequisite before validation/daemon commands.
- Updated `docs/plan.md` with explicit rollout steps (including bootstrap as step 2) and a current-state TODO checklist marking completed milestones vs remaining implementation items.
- Prioritized remaining plan TODOs and completed top-priority wiring: daemon now executes strategy actions through offer-build command contract, posts successful offers to Dexie, persists posted offer IDs to offer-state, and records strategy offer execution audit events.
- Implemented in-process `chia-wallet-sdk` offer builder module (`greenfloor/cli/offer_builder_sdk.py`) used by daemon strategy offer execution, with deterministic tests covering successful offer encoding and failure contracts.
- Implemented policy-gated cancel execution path in daemon for unstable-leg markets: compares prior/current XCH snapshots, requires strong move threshold, cancels only open offers, persists cancelled state, and emits `offer_cancel_policy` audit events.
- Added `docs/runbook.md` with operator-first deployment, recovery/rollback, audit monitoring, and incident triage workflows; marked plan `P3` complete.
- Started a new `V1.1 Backlog (Draft)` section in `docs/plan.md` with prioritized follow-on items for reconciliation, policy controls, observability, retries, and integration hardening.
- Simplified “running state first” path further: promoted manager `build-and-post-offer` as the primary operator command (mainnet default, optional testnet override), with updated tests and runbook guidance.
- Added `build-and-post-offer --dry-run` preflight mode to build/validate offers without posting to Dexie, including test coverage and runbook guidance.
- Added pair-based market selection for `build-and-post-offer` (`--pair base:quote` or `base/quote`) with deterministic ambiguity guardrails and `--market-id` fallback for duplicate-pair markets.
- Added `supported_assets_example` defaults in `config/markets.yaml` for 5 carbon assets from `old/common.py`, enriched with current Dexie ticker metadata (`ticker_id`, `pool_id`, `last_price_xch`) for fast operator reference.
- Added manager `list-supported-assets` command to print `supported_assets_example` from markets config as JSON for operator workflows; added deterministic tests and runbook mention.
- Expanded `supported_assets_example` defaults to include quote-side assets `XCH` (native) and `wUSDC.b` (Dexie token id: `fa4a...a99d`) so pair-based operator workflows have both base and quote examples in one place.
- Added `$BYC`, `$MRMT`, and `$SBX` to `supported_assets_example` using Dexie token IDs for faster pair-driven operator setup.
- Manager now auto-resolves default config paths from `~/.greenfloor/config/*.yaml` when present (fallback to repo `config/*.yaml`), so `--markets-config`/`--program-config` are primarily override flags.
- Reworked offer publishing venue selection: Splash is now a first-class alternative (not fallback), selected via `venues.offer_publish.provider` in `program.yaml` with CLI one-off override support (`--venue dexie|splash`).
- Reviewed `chia-wallet-sdk` test surfaces and confirmed simulator-backed coverage in upstream Rust tests; added a lightweight default GreenFloor simulator smoke harness (offer make/take path) plus opt-in extended checks for explicit key/spend + offer flows (`tests/test_chia_wallet_sdk_simulator_harness.py`).
- Updated default simulator harness to run four fast/high-signal CAT-centric upstream Rust tests by default (CAT issue, CAT send-with-change, CAT primitive spend, CAT offer catalog/action-layer), with heavier checks kept in opt-in full mode.
- Added manager-side offer reconciliation pass and compact offer status views for operator testing: `offers-reconcile` now refreshes persisted `offer_state` by offer-id (Dexie lookup with orphan/unknown flagging) and records `offer_reconciliation` audit events; `offers-status` summarizes current offer-state counts plus recent strategy/lifecycle/reconciliation events.
- Added deterministic multi-cycle daemon integration harness (`tests/test_daemon_multi_cycle_integration.py`) covering price-shifted planning and posting on cycle 1, cancel-policy trigger on strong unstable-leg move on cycle 2, and manager reconciliation verification against persisted offer-state.
- Implemented bounded retry/backoff + cooldown contracts for daemon offer post/cancel execution with explicit reason codes (`*_retry_exhausted`, `*_cooldown_active`), env-tunable controls (`GREENFLOOR_OFFER_POST_*`, `GREENFLOOR_OFFER_CANCEL_*`), and deterministic retry/cooldown tests.
- Implemented config-driven strategy spread/price-band controls (B2): daemon now reads `strategy_target_spread_bps`, `strategy_min_xch_price_usd`, and `strategy_max_xch_price_usd` from market pricing config and propagates those into planning/payloads; added deterministic strategy/daemon tests and sample config keys in `config/markets.yaml`.
- Implemented metrics export (B5): daemon now emits per-cycle `daemon_cycle_summary` timing/error aggregates, and manager `metrics-export` outputs counts/latency/error rates for daemon cycles, offer execution, cancel policy, and error events from SQLite audit history.
- Implemented configuration schema validation hardening (B8): markets config parsing now validates strategy pricing controls (`strategy_target_spread_bps`, min/max XCH price band with min<=max), and manager `doctor` now warns on invalid runtime env override values for offer retry/cooldown and cancel-threshold controls.
- Aligned cancel-policy execution with plan semantics by requiring explicit stable-vs-unstable market eligibility (`pricing.cancel_policy_stable_vs_unstable: true`) in addition to unstable-leg gating; added deterministic tests and runbook guidance plus a golden-path smoke-test checklist for operator user testing.
- Reaffirmed manager offer-builder on `chia-wallet-sdk` path (`greenfloor/cli/offer_builder_sdk.py`) with deterministic unit coverage and runbook updates for `testnet11` on-chain asset bring-up planning.
- Reviewed plan/progress for current-state accuracy and captured remaining pre-upstream gaps explicitly: coin-backed SDK offer construction for venue-valid posting, `testnet11` asset bootstrap helper workflow, and first documented live `testnet11` proof path.
- Added an explicit upstreaming checklist section to `docs/plan.md` covering GitHub repo creation, remote push, branch protection, required checks, Actions/secret hygiene, and first PR verification flow.

## 2026-02-19

- Initialized GreenFloor v1 implementation scaffold.
- Added Python package structure with `greenfloor-manager` and `greenfloord` entrypoints.
- Implemented config parsing/validation and low-inventory alert evaluation core logic.
- Added key routing validation and a Pushover notification adapter.
- Added baseline project hygiene files (`.gitignore`, pre-commit, CI workflows, pyright config).
- Added initial `AGENTS.md` and architecture decision record folder.
- Added manager command to register Coinset tx-block webhook callback endpoints.
- Added SQLite persistence for alert state, audit events, and startup price-policy snapshots.
- Added manager commands for manual consolidate flow and low-watermark config edits.
- Wired daemon to persist alert state across runs and record Coinset mempool snapshot/error audit events.
- Added daemon reload marker helpers and a long-running loop mode with reload-marker consumption.
- Added Coinset tx-block webhook listener scaffold that stores webhook payloads as audit events.
- Verified manager/daemon smoke commands using `.venv` Python and passing test suite (`10 passed`).
- Added manager `set-price-policy` command to update YAML pricing and persist immutable before/after history in SQLite.
- Added tx signal tracking (`mempool_observed`/`tx_block_confirmed`) persistence paths and tests.
- Added deterministic offer lifecycle state machine module with transition tests.
- Added ladder-aware coin-ops planning module (split/combine with fee and op caps) and tests.
- Replaced daemon placeholder coin bucket logic with market-config ladder + bucket-count driven planning.
- Added Dexie-offer driven lifecycle persistence (`offer_state`) and wallet-adapter dry-run execution hooks for planned coin ops.
- Added inventory bucket scanning via wallet adapter boundary (with deterministic env-backed stub) plus fallback to config seed counts.
- Extended config models to include runtime dry-run, venue base URLs, coin-op execution limits, ladders, pricing, and inventory bucket-count maps.
- Wired daemon lifecycle transitions from live Dexie offer status snapshots and persisted offer state updates.
- Added optional `chia_wallet_sdk` coin-record query path in wallet adapter (auto-fallback to deterministic env stub when unavailable).
- Hardened asset-scan policy: CAT inventory now defaults to explicit CAT mapping fallback (to avoid unsafe unfiltered coin scans), while XCH can use live SDK receive-address scanning.
- Added structured coin-op audit event emission (`coin_op_planned`/`coin_op_executed`/`coin_op_skipped`) derived from adapter execution items.
- Added `greenfloor-manager doctor` readiness command covering config, key routing, SQLite writeability, webhook address sanity, and Pushover env checks.
- Added daily fee-budget enforcement for coin-op execution with projected-fee checks, explicit skip audits, and coin-op ledger accounting in SQLite.
- Upgraded fee-budget behavior from all-or-skip to partial execution: execute in-priority ops that fit budget and mark overflow ops as skipped with explicit reason codes.
- Added manager `coin-op-budget-report` command with UTC daily ledger summaries (spent, executed/planned/skipped, fee-budget-overflow skipped ops).
- Added manager `set-ladder-entry` command for per-market/per-side bucket tuning (target_count, split buffer, combine excess factor) with tests.
- Added manager `set-bucket-count` command for direct inventory bucket-count tuning and optional immediate reload marker signaling.
- Added optional `--reload` support to ladder updates so manager config edits can trigger daemon reload workflow directly.
- Added atomic, versioned YAML config editing path with `.history` backups and checksum metadata for manager-driven config mutations.
- Added manager config history tooling: list versioned YAML snapshots and safely revert to selected backups with optional reload signaling and SQLite audit events.
- Added config-history `--latest` revert convenience and guardrails to ensure backup files match the target config history namespace.
- Added rollback safety confirmation prompt by default for config-history revert, with `--yes` override for automation/non-interactive workflows.
- Added key onboarding flow (`keys-onboard`) that prefers discovered `~/.chia_keys` and falls back to mnemonic import or new-key generation, with persisted onboarding selection state.
- Wired daemon coin-op execution to onboarding signer selection (`key_id`/network/source) and enforced signer-context checks in wallet adapter execution path.
- Added executor chain for signer-routed coin-op execution: wallet executor -> `chia_keys` executor -> passthrough -> worker -> signer.
- Added built-in `chia_keys` executor broadcast path: if passthrough returns `spend_bundle_hex`, executor decodes and submits via `chia_wallet_sdk` RPC client and returns tx hash as operation id.
- Added built-in passthrough/worker/signer command contracts with deterministic validation and explicit env-var override points for backend integration.
- Added built-in signer-backend command that performs XCH coin discovery + selection (`chia_wallet_sdk`) and forwards a deterministic builder request contract for spend-bundle construction/signing.
- Added built-in builder command (`chia_keys_builder`) as default signer-backend builder target, with validated bundle-signing request contract and explicit `GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD` hook for real spend-bundle generation.
- Added built-in bundle-signer command (`chia_keys_bundle_signer`) as default builder signer target, with validated raw-signing request contract and explicit `GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD` hook for final spend-bundle signing.
- Added built-in raw bundle-signer command (`chia_keys_bundle_signer_raw`) as default raw signer target, with validated engine request contract and explicit `GREENFLOOR_CHIA_KEYS_RAW_ENGINE_CMD` hook for final spend-bundle generation.
- Added built-in raw engine command (`chia_keys_raw_engine`) as default raw engine target, with validated signing job contract and explicit `GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_CMD` hook for final spend-bundle signing.
- Added built-in raw-engine-sign command (`chia_keys_raw_engine_sign`) as default raw-engine sign target, with keyring-first validation and explicit `GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD` hook for final spend-bundle signing implementation.
- Added built-in raw-engine-sign implementation command (`chia_keys_raw_engine_sign_impl`) as default sign-impl target, with deterministic split/combine tx-output planning and explicit `GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD` hook for final spend-bundle signing output.
- Added built-in SDK-submit signer command (`chia_keys_raw_engine_sign_impl_sdk_submit`) as default sign-impl signer target, with selected-coin/addition payload mapping and spend-bundle extraction from submit command responses.
- Removed `chia rpc` signing path from active signer chain and replaced it with SDK-submit naming/hooks end-to-end (`sdk_submit_*` reason contracts).
- Added no-regression test (`test_no_chia_rpc_signing_path.py`) to fail if active signer pipeline reintroduces `chia rpc wallet` command usage.
- Added/updated tests for onboarding persistence, wallet execution routing, executor delegation, passthrough worker contract, signer contract, and spend-bundle broadcast handling.
- Implemented in-process `sdk_submit` signing path: loads key material from `~/.chia_keys` via Chia keychain APIs, derives synthetic wallet keys, builds XCH split/combine `SpendBundle` with `chia_wallet_sdk`, and signs AGG_SIG targets without Chia RPC usage; retains explicit override hook via `GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD`.
- Added end-to-end wallet-executor chain test that exercises the default subprocess pipeline (`wallet_executor` -> `chia_keys_*`) with a fake SDK module and sdk-submit override, verifying successful spend-bundle broadcast flow and operation-id propagation.
- Added end-to-end failure-path chain test for default subprocess pipeline to verify `no_unspent_xch_coins` reason propagation from signer-backend through executor layers to top-level wallet executor output.
- Added end-to-end failure-path chain test for `coin_selection_failed:*` propagation (SDK `select_coins` exception) through default subprocess pipeline to top-level wallet executor output.
- Added end-to-end failure-path chain test for broadcast rejection propagation (`broadcast_failed:*`) when executor push-tx returns unsuccessful status.
- Added reason-propagation test for malformed passthrough output to assert top-level `passthrough_invalid_json` handling in wallet-executor path.
- Updated v1 plan language to align onboarding contract with manager-first behavior: daemon startup no longer requires completion of interactive first-run interview when equivalent validated config/key references already exist.
- Hardened signer-key resolution contract with repo-managed key registry in `program.yaml` (`keys.registry`), router validation against registry/network, daemon propagation of resolved key fingerprint into signer subprocess env mapping, and manager doctor/config validation coverage for missing registry mappings.

# Progress Log

## 2026-02-25 (step 4 monitoring, reliability, tests, runbooks)

- Implemented Step 4 monitoring/reliability hardening in `greenfloor/cli/manager.py`:
  - Added moderate retry/backoff handling for transient poll failures in signature status checks, wallet-offer artifact polling, and coin-list polling loops.
  - Extended signature wait diagnostics with additive soft-timeout escalation events (`signature_wait_warning`, `signature_wait_escalation`) while continuing to wait on user signing.
  - Extended mempool/confirmation diagnostics to emit richer wait metadata (`wait_reason`, coin ids/names) and include Coinset links on `in_mempool` user events.
  - Added read-only Coinset reconciliation metadata on mempool/confirmation events (`confirmed_block_index`, `spent_block_index` when available).
  - Added post-confirmation reorg-risk monitoring (`reorg_watch_*`) that watches six additional blocks before declaring coin-op wait completion.
- Added Coinset adapter support for reorg watch peak-height reads:
  - `greenfloor/adapters/coinset.py` now exposes `get_blockchain_state()` for read-only chain-height reconciliation.
- Added canonical taker detection instrumentation during offer reconciliation:
  - `offers-reconcile` now emits `taker_signal` based on canonical offer-state transitions and `taker_diagnostic` based on advisory status patterns.
  - Added `taker_detection` audit events and included them in `offers-status` recent event output.
- Added deterministic tests:
  - `tests/test_manager_post_offer.py` now covers signature escalation/retry behavior, mempool wait diagnostics with reorg-watch stubs, and reorg-watch depth waiting logic.
  - `tests/test_manager_offer_reconcile.py` now validates taker-signal fields and `taker_detection` audit event emission.
- Updated operator docs:
  - `README.md` and `docs/runbook.md` now document new wait/retry/reorg/taker diagnostics and expected event contracts.
- Validation snapshot:
  - `.venv/bin/python -m pytest tests/test_manager_post_offer.py tests/test_manager_offer_reconcile.py` -> `76 passed`

## 2026-02-25 (step 3 strict-close canonical pair proof)

- Closed the canonical pair mapping gap for manager Cloud Wallet posting in `greenfloor/cli/manager.py`:
  - `build-and-post-offer --pair CARBON22:xch` now resolves canonical market asset IDs (`CAT` hex tail and `xch`) to Cloud Wallet global asset IDs (`Asset_...`) before `createOffer`.
  - CAT metadata validation now uses Dexie token metadata as the primary source; Cloud Wallet in-vault candidate ranking remains a temporary fallback selector until wallet APIs expose canonical CAT-tail metadata directly.
  - Added explicit result metadata (`resolved_base_asset_id`, `resolved_quote_asset_id`) to manager output and strategy-offer audit events.
  - Added direct state persistence in cloud-wallet manager post path (`offer_state` upsert + `strategy_offer_execution` audit event) so follow-up `offers-status` / `offers-reconcile` can observe posted offers.
- Live canonical proof (remote host `John-Deere`) succeeded:
  - command: `GREENFLOOR_COINSET_BASE_URL=\"https://api.coinset.org\" greenfloor-manager ... build-and-post-offer --pair CARBON22:xch --size-base-units 1`
  - `signature_request_id`: `SignatureRequest_gqxapuzsb1yectpxnyblci28`
  - venue post success on Dexie with offer id `EEn9gzNvg6a34jCsRhJZpJifW3FGXFy15VXkw6tzg48s`
  - `publish_failures: 0`
  - maker fee contract held: `offer_fee_mojos: 0`, `offer_fee_source: "maker_default_zero"`
  - canonical request pair remained `CARBON22:xch` while resolved IDs were emitted in output.
- Lifecycle visibility now works after direct manager post:
  - `offers-status` showed persisted rows including `EEn9gzNvg6a34jCsRhJZpJifW3FGXFy15VXkw6tzg48s` and `strategy_offer_execution` audit payload with resolved asset IDs.
  - `offers-reconcile` reconciled persisted rows (`reconciled_count: 2`) and transitioned state based on current Dexie lookup responses.

## 2026-02-25 (step 3 live proof: cloud wallet maker offer posted)

- Executed live Step 3 proof for Cloud Wallet maker flow on mainnet market `carbon_2022_xch_sell`.
- Environment and config fixes applied before proof:
  - initialized `chia-wallet-sdk` submodule and installed SDK Python binding into project venv,
  - installed `greenfloor-native` after upgrading Rust toolchain (to satisfy Cargo lockfile v4),
  - switched Dexie endpoint to mainnet (`https://api.dexie.space`),
  - used explicit Coinset API host override for this run: `GREENFLOOR_COINSET_BASE_URL=https://api.coinset.org`,
  - updated market receive address to a valid lowercase mainnet bech32 address.
- Cloud Wallet asset-ID compatibility update used for this proof run:
  - `base_asset` changed to Cloud Wallet global ID `Asset_vznqpopp6sp3s0qwkuvua3dp`,
  - `quote_asset` changed to Cloud Wallet global ID `Asset_huun64oh7dbt9f1f9ie8khuw`.
- Successful live command:
  - `GREENFLOOR_COINSET_BASE_URL="https://api.coinset.org" greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml build-and-post-offer --pair CARBON22:Asset_huun64oh7dbt9f1f9ie8khuw --size-base-units 1`
- Proof result:
  - `signature_request_id`: `SignatureRequest_mpve6gp6gw87oa4pshpntste`
  - `signature_state`: `SUBMITTED`
  - venue post: success on Dexie, offer id `EU6M6FcwF279prpvkXcNYgdFJv7fmyLujg9FDMxVqVyp`
  - `publish_failures`: `0`
  - maker fee contract verified: `offer_fee_mojos: 0`, `offer_fee_source: "maker_default_zero"`
  - local verification path passed (publish reached venue and returned success).
- Post-run manager views:
  - `offers-reconcile` and `offers-status` returned empty sets in this direct manager path run (`offer_count: 0`), indicating no persisted offer-state rows were available for reconciliation from this invocation.

## 2026-02-24 (step 3 fee-path simplification follow-up)

- Simplified coin-operation fee policy to remove env/cache fallback complexity:
  - `greenfloor/cli/manager.py` now takes fee advice only from Coinset conservative estimates, with bounded retries/backoff.
  - If Coinset advice is unavailable, fallback is now only `coin_ops.minimum_fee_mojos` from program config (supports `0`).
  - If Coinset returns a fee lower than configured minimum, manager applies the minimum floor.
  - Removed legacy `GREENFLOOR_COINSET_ADVISED_FEE_MOJOS` and in-memory last-good cache fallback behavior from the active path.
- Added config model/schema support:
  - `greenfloor/config/models.py` now parses `coin_ops.minimum_fee_mojos` into `ProgramConfig.coin_ops_minimum_fee_mojos` with validation (`>= 0`).
  - `config/program.yaml` now includes `coin_ops.minimum_fee_mojos: 0` in the default template.
- Updated tests/docs to match the simplified contract:
  - `tests/test_manager_post_offer.py` now covers coinset-success, minimum-floor, and config-minimum fallback cases.
  - Error guidance for fee-resolution failures now references `coin_ops.minimum_fee_mojos` instead of env overrides.
  - `tests/test_low_inventory_alerts.py` updated for the extended `ProgramConfig` shape.
  - `README.md` operator override note updated to document config-based minimum fallback.
- Validation snapshot:
  - `.venv/bin/python -m pytest tests/test_manager_post_offer.py tests/test_low_inventory_alerts.py` -> `74 passed`
  - `.venv/bin/python -m ruff check greenfloor/cli/manager.py greenfloor/config/models.py tests/test_manager_post_offer.py tests/test_low_inventory_alerts.py` -> pass
  - `.venv/bin/python -m ruff format --check greenfloor/cli/manager.py greenfloor/config/models.py tests/test_manager_post_offer.py tests/test_low_inventory_alerts.py` -> pass

## 2026-02-24 (cloud wallet vault-first step 3 implementation)

- Note: fee fallback details in this entry were superseded by the follow-up entry `2026-02-24 (step 3 fee-path simplification follow-up)`.

- Implemented Step 3 offer-post hardening in `greenfloor/cli/manager.py`:
  - Added artifact polling loop (`_poll_offer_artifact_until_available`) with timeout and moderate exponential backoff so Cloud Wallet maker flow waits for a newly produced signed `offer1...` artifact instead of taking a single wallet snapshot.
  - Artifact selection now excludes pre-existing offers by tracking pre-create offer markers (`offerId`/`bech32`) and selecting newly materialized artifacts only.
  - Added shared fee resolver contract (`_resolve_operation_fee`) and role wrappers:
    - maker create-offer path resolves to explicit zero fee (`maker_default_zero`),
    - taker/coin operations keep Coinset conservative advice with retries and TTL-bounded last-good fallback.
  - Tightened fee fallback semantics to match plan contract: retry Coinset advice first, then fallback only when cached advice is still within TTL; stale cache now fails with actionable operator guidance.
  - Cloud-wallet build/post output now includes `offer_fee_source` alongside `offer_fee_mojos`.
- Added deterministic test coverage in `tests/test_manager_post_offer.py`:
  - new artifact polling success and timeout tests (mocked `time.sleep` + `time.monotonic`),
  - fee resolver tests for retry-then-cache fallback and stale-cache rejection,
  - cloud-wallet build/post tests updated to stub artifact polling helper and assert timeout error contract.
- Validation snapshot:
  - `.venv/bin/python -m pytest tests/test_manager_post_offer.py` -> `70 passed`
  - `.venv/bin/python -m ruff check greenfloor/cli/manager.py tests/test_manager_post_offer.py` -> pass
  - `.venv/bin/python -m ruff format --check greenfloor/cli/manager.py tests/test_manager_post_offer.py` -> pass

## 2026-02-25 (Step 2 simplification pass: PR #25)

Reviewed commit `a7614875` (Step 2 closure) and identified six categories of issues;
addressed all of them in branch `simplify/step2-coin-prep-cleanup` (PR #25).

**Code simplifications and correctness fixes (`manager.py`):**

- Extracted `_resolve_coin_global_ids(wallet_coins, raw_ids) -> (resolved, unresolved)` helper to
  eliminate a verbatim ~22-line coin name→`Coin_*` mapping block that was copy-pasted identically
  into both `_coin_split` and `_coin_combine`.
- Fixed `_coins_list` spendability field: was using a blocklist (`not in {"SPENT", "PENDING",
"MEMPOOL"}`), marking unknown/transitional states as spendable. Switched to `_is_spendable_coin`
  (allowlist) to match what the denomination readiness evaluator counts.
- Fixed `_wait_for_mempool_then_confirmation` re-warning intervals: the function was doubling its
  own warning threshold on each emission (`mempool_warning_seconds += mempool_warning_seconds`),
  making warnings exponentially rare (5min → 10min → 20min → ...). The signature poll loop uses
  fixed-interval additive re-warning; aligned both loops with `next_mempool_warning` and
  `next_confirmation_warning` accumulators that advance by the original interval.

**Test infrastructure (`test_manager_post_offer.py`):**

- Extracted `_write_program_with_cloud_wallet(path, *, provider)` helper, eliminating 13
  identical 5-line inline cloud wallet credential patching blocks (~80 lines removed).

**Missing test coverage added (26 new tests, 39→65 in this file):**

- `_is_spendable_coin`: allowlist states, known non-spendable, unknown states, missing state.
- `_resolve_coin_global_ids`: name→global-id mapping, `Coin_*` pass-through, unresolved reporting.
- `_evaluate_denomination_readiness`: spendability+asset+amount filtering, min/max bounds,
  case-insensitive asset matching.
- `_poll_signature_request_until_not_unsigned`: immediate return, warning event emission, timeout.
- `_wait_for_mempool_then_confirmation`: in_mempool event with coinset URL, confirmed return,
  mempool warning event, initial-coin-id filtering.
- Cloud wallet dispatch gate: dispatches when all four config fields present + not dry_run;
  bypasses cloud wallet on dry_run even when configured.
- `_build_and_post_offer_cloud_wallet` direct paths: happy path (poll→artifact→verify→post),
  no-artifact error, verify-error blocks post.
- `until_ready` success path: `stop_reason="ready"` with exit code 0.
- `_coin_combine` `requires_new_coin_selection` stop reason with explicit `--coin-id` in
  `--until-ready` mode.

Validation snapshot: `196 passed, 5 skipped`; pre-commit all hooks pass.

**Simulator harness removed (`tests/test_chia_wallet_sdk_simulator_harness.py`, `ci.yml`):**

- Deleted `tests/test_chia_wallet_sdk_simulator_harness.py` (122 lines): all six tests ran
  `cargo test` on `chia-sdk-driver` Rust internals (CAT issuance, CAT send, CAT catalog,
  managed reward distributor, spend_simulator example) and tested no GreenFloor code. The SDK
  has its own CI.
- Removed the corresponding Ubuntu-only CI step from `.github/workflows/ci.yml`.
- Retained `test_greenfloor_native_integration.py` (validates `validate_offer` +
  `from_input_spend_bundle_xch` round-trip via `greenfloor-native`), which is the correct
  boundary test for SDK surface GreenFloor actually uses.

## 2026-02-24 (mainnet Step 2 operator proof: CARBON22:xch)

- Executed end-to-end Step 2 coin-prep proof on mainnet pair `CARBON22:xch` using Cloud Wallet vault `Wallet_le99o1k4jfsof9mp817gxpi3`.
- Baseline inventory check (`coins-list`) confirmed live settled spendable inventory and coin-id visibility before prep actions.
- Ran split prep with readiness mode:
  - `greenfloor-manager coin-split --pair CARBON22:xch --coin-id 2f264eb91017f196596ee7a6635ff3d298a295226fbd1a57cb6b7493aefa3c34 --size-base-units 10 --until-ready --max-iterations 3`
  - Result included `signature_state: "SUBMITTED"`, mempool signal, and coinset link:
    - `https://coinset.org/coin/63f7016c704eb0d6cdaf6fec0a6d2189c2e363b3b19d87e623cc36029fa06bcd`
  - Post-split inventory showed three new settled `amount=10` coins:
    - `75f3f88bed96681808b71a9558f4fe29017ecb42e146d9fdba01804dfd9a3548`
    - `1bcbf190fe928a4c6485b41cab0dc3c535be7a96bd89f8edfb54c34d61c002b3`
    - `02ba4471f248875bdf314e4adb9eda683253848a1b6348a243b489a1033ee9c1`
- Ran combine prep with explicit coin IDs:
  - `greenfloor-manager coin-combine --pair CARBON22:xch --coin-id 75f3f88bed96681808b71a9558f4fe29017ecb42e146d9fdba01804dfd9a3548 --coin-id 1bcbf190fe928a4c6485b41cab0dc3c535be7a96bd89f8edfb54c34d61c002b3 --coin-id 02ba4471f248875bdf314e4adb9eda683253848a1b6348a243b489a1033ee9c1 --number-of-coins 3 --asset-id Asset_huun64oh7dbt9f1f9ie8khuw`
  - Result included `signature_state: "SUBMITTED"`, mempool signal, and coinset link:
    - `https://coinset.org/coin/76e4cd84f745abaa8f93fe5fbc10115d5a086dc95060c9ca1e08d320c20c3984`
- Final inventory check confirmed combine settlement:
  - prior three `amount=10` coins were consumed,
  - new settled coin `76e4cd84f745abaa8f93fe5fbc10115d5a086dc95060c9ca1e08d320c20c3984` with `amount=30` present.
- Fee contract behavior during proof:
  - `fee_mojos: 0`
  - `fee_source: "env_override"`

## 2026-02-24 (pr24 review follow-up hardening)

- Applied post-review hardening updates for PR #24:
  - Coin-prep venue is now optional metadata (`--venue` validates only when explicitly provided); split/combine no longer depend on `offer_publish_venue`.
  - Readiness classification is now conservative: only known spendable states count toward readiness, and unknown/transitional states are treated as not spendable.
  - Readiness asset parsing now supports both `asset: {id: ...}` and `asset: "<id>"` payload forms.
  - Ladder combine threshold now uses `ceil(target_count * combine_when_excess_factor)` (minimum 2) instead of truncation.
  - Coin-prep output now includes `coin_selection_mode` so operator intent is explicit (`explicit` vs adapter-managed auto-select).
- Added deterministic tests covering:
  - optional venue behavior on coin-prep output,
  - ceil-based combine threshold derivation,
  - readiness filtering for unknown states and string-form asset IDs.
- Updated runbook notes for readiness-loop usage and explicit `--coin-id` interaction in `--until-ready` mode.
- Validation snapshot:
  - `tests/test_manager_post_offer.py`: `39 passed`.
  - `pre-commit`: all hooks pass except existing `pyright` failures in legacy `old/` scripts.

## 2026-02-24 (step 2 closure: readiness loop + boundary alignment)

- Closed remaining Step 2 gaps for Vault-first coin prep:
  - Added readiness-loop mode for `coin-split` and `coin-combine` in `greenfloor/cli/manager.py` with `--until-ready` and bounded retries via `--max-iterations`.
  - Loop mode evaluates denomination readiness from live vault inventory after each operation and returns explicit stop reasons (`ready`, `max_iterations_reached`, `requires_new_coin_selection`).
  - Kept and documented direct boundary for coin prep operations: `manager.py` calls `greenfloor/adapters/cloud_wallet.py` directly for split/combine.
- Added deterministic manager tests for readiness-loop validation and not-ready loop behavior in `tests/test_manager_post_offer.py`.

## 2026-02-24 (live testing retarget to mainnet CARBON22)

- Retargeted active live testing from `testnet11` proof pair context to mainnet `CARBON22:xch`.
- Confirmed `CARBON22` CAT ID from repo config (`config/markets.yaml`): `4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7`.
- Updated planning docs to reflect current live target while preserving prior `testnet11` proof artifacts as historical evidence.

## 2026-02-24 (cloud wallet vault-first step 2 completion)

- Completed Step 2 alignment for Vault-first coin preparation in `greenfloor/cli/manager.py`:
  - `coin-split` and `coin-combine` now accept `--venue` so coin-prep runs are explicitly bound to the selected downstream posting venue context (`dexie` or `splash`).
  - Added config-driven denomination mode for both commands via `--size-base-units`, sourced from the selected market `ladders.sell` entry.
  - In config-driven split mode, manager derives/enforces `amount_per_coin` and `number_of_coins` from ladder `size_base_units` + `target_count` + `split_buffer_count`.
  - In config-driven combine mode, manager derives/enforces `number_of_coins` from ladder `target_count * combine_when_excess_factor` (minimum 2) and defaults `asset_id` to the market base asset.
  - Command JSON output now includes resolved venue and resolved denomination target metadata to make prep decisions explicit and auditable.
- Added deterministic coverage in `tests/test_manager_post_offer.py` for config-driven split/combine behavior and venue binding output.
- Updated operator docs in `docs/runbook.md` with config-driven coin-shaping examples.

## 2026-02-24 (cloud wallet mainnet operator hardening)

- Improved Cloud Wallet config discoverability in operator docs/templates:
  - Added explicit source hints in `docs/runbook.md` for `cloud_wallet.base_url`, `cloud_wallet.user_key_id`, `cloud_wallet.private_key_pem_path`, and `cloud_wallet.vault_id`.
  - Added inline comments in `config/program.yaml` showing where to fetch each value and common pitfalls (origin-only `base_url`, PEM format, `Wallet_...` vs launcher id).
- Hardened coin-operation fee-advice failure behavior in `greenfloor/cli/manager.py`:
  - `coin-split` and `coin-combine` now return structured JSON failure payloads with operator guidance instead of uncaught tracebacks when fee resolution fails.
  - Added deterministic regression tests in `tests/test_manager_post_offer.py` for both commands.
- Fixed Cloud Wallet split ID mismatch in `greenfloor/cli/manager.py`:
  - `coin-split` now resolves operator-provided coin ids from `coins-list` output (hex coin names) to required GraphQL `Coin_*` global IDs before mutation submission.
  - Added structured `coin_id_resolution_failed` response when requested ids are not present in vault inventory, plus deterministic test coverage.
- Mainnet smoke validation succeeded after patch:
  - `coin-split --no-wait` produced a valid `signature_request_id` and returned `UNSIGNED` as expected for async mode.
  - Operator confirmed Chia Signer prompt/approval path completed successfully with `fee_source: "env_override"` and `fee_mojos: 0`.
- Improved manager JSON output ergonomics in `greenfloor/cli/manager.py`:
  - Default JSON command output is now pretty-formatted for operator readability.
  - Added global `--json` flag for compact single-line JSON output in script/automation workflows.
- Added exact-coin targeting support to `coin-combine`:
  - New repeatable `--coin-id` argument resolves `coins-list` hex names to Cloud Wallet `Coin_*` IDs and submits targeted combine requests.
  - Added deterministic regression tests for successful ID resolution and structured unknown-ID error responses.
- Mainnet exact-coin combine validation succeeded:
  - Operator combined three specified micro-coins into a single resulting coin `83a4841f6b7992f20876f10ff92b0cab69a5f3f988cd1e7918a4d41ca11f1b12` with signer approval.

## 2026-02-24 (cloud wallet vault-first migration pass)

- Added Cloud Wallet adapter boundary in `greenfloor/adapters/cloud_wallet.py`:
  - GraphQL request transport with user-key RSA-SHA256 auth headers (`chia-user-key-id`, `chia-signature`, `chia-nonce`, `chia-timestamp`).
  - Vault coin listing, split/combine mutation calls, offer creation, signature-request polling, and wallet-offer retrieval.
- Extended program config model with Cloud Wallet fields:
  - `cloud_wallet.base_url`
  - `cloud_wallet.user_key_id`
  - `cloud_wallet.private_key_pem_path`
  - `cloud_wallet.vault_id`
- Added manager vault-first commands:
  - `coins-list` (operator-minimal coin fields: coin id, amount, state, pending, spendable, asset),
  - `coin-split` (default wait through signature + mempool + confirmation, `--no-wait` override),
  - `coin-combine` (default wait through signature + mempool + confirmation, `--no-wait` override).
- Added Coinset conservative fee-advice contract for mempool-bound coin operations:
  - retry with moderate exponential backoff,
  - cache last-good fee with TTL fallback,
  - actionable failure if no fee advice is available.
- Switched manager offer-post default path to Cloud Wallet when cloud-wallet config is present:
  - maker fee forced to `0`,
  - signature-request wait handling with periodic warnings,
  - local offer verification retained before venue post.
- Added/updated deterministic tests in `tests/test_manager_post_offer.py` for the new cloud-wallet command behavior and fee wiring.

## 2026-02-24 (testnet pair pivot to TDBX)

- Updated active `testnet11` proof target to `TDBX:txch` (TXCH<->TDBX) for workflow-driven validation.
- Reverted BYC04 test market activation in `config/markets.yaml` (`byc04_txch_sell` back to `enabled: false`) so unsupported-liquidity pairs are not selected as active proof markets.
- Refreshed operator-facing examples to align on the canonical testnet proof pair:
  - `README.md` testnet publish example now uses `TDBX:txch`.
  - `docs/runbook.md` steady-state and golden-path testnet commands now use `TDBX:txch`.
- Updated `docs/plan.md` G3 note to explicitly record `TDBX:txch` as the active `testnet11` operator proof pair.

## 2026-02-24 (offer expiry enforcement)

- Added Dexie pre-submit verification enforcement in `greenfloor/cli/manager.py` to reject offers that do not contain at least one `ASSERT_BEFORE_*` expiration condition (time or block-height).
- Added regression coverage in `tests/test_manager_post_offer.py` for:
  - offer verification failure when decoded offer conditions include no expiration assertion,
  - end-to-end `build-and-post-offer` behavior that blocks publish and returns a non-zero code when the offer has no expiry, treating that block as expected/successful test behavior.
- Updated `docs/plan.md` Offer File Contract to state the enforced expiration requirement explicitly.

## 2026-02-23 (G2 testnet bootstrap helper workflow)

- Implemented minimal operator helper workflow for G2: `.github/workflows/testnet11-asset-bootstrap-helper.yml`.
- Added explicit workflow-dispatch inputs for the G2 shape:
  - `network_profile` (must be `testnet11`),
  - `dexie_base_url`,
  - `quote_symbol` (must be `txch`),
  - `top_n_assets`,
  - `signer_key_id`,
  - `receive_address`,
  - optional `include_asset_ids_csv`.
- Added a deterministic discovery/artifact contract under `artifacts/g2-testnet-asset-bootstrap/`:
  - `raw-tokens.json`: raw Dexie `/v1/swap/tokens` payload capture,
  - `normalized-tokens.json`: normalized+ranked CAT candidates,
  - `selected-assets.json`: selected candidate set for bootstrap,
  - `markets-snippet.yaml`: copy/paste starter snippet for `supported_assets_example` and disabled `markets` stanzas,
  - `summary.md`: run summary with selected-asset quick view.
- Added artifact upload for easy operator download: `g2-testnet-asset-bootstrap-artifacts`.

## 2026-02-23 (workflow install-path alignment)

- Audited all repository workflows for `chia-wallet-sdk` install strategy (`.github/workflows/ci.yml`, `.github/workflows/live-testnet-e2e.yml`).
- Confirmed both workflows are now aligned on the same efficient pattern:
  - resolve pinned submodule commit SHA (`git rev-parse HEAD:chia-wallet-sdk`),
  - restore cache for `./.cache/wheelhouse/chia-wallet-sdk` keyed by `{os, arch, sha}`,
  - build wheel from `./chia-wallet-sdk/pyo3` only on cache miss,
  - install from cached wheelhouse artifact.
- Applied consistency cleanup in CI workflow:
  - renamed cache step labels/IDs and key prefix to `sdk-wheelhouse-*` so CI and live workflow conventions match exactly.

## 2026-02-23 (native branch live-proof refresh)

- Re-validated live `testnet11` manager proof on current native-migration head (`cb41976`) using GitHub Actions:
  - Workflow: `Live Testnet E2E (Optional)` (`run_id=22325053517`), `workflow_dispatch`, branch `feat/greenfloor-native-upstream-migration`.
  - Inputs: `pair=TDBX:txch`, `size_base_units=1`, `dry_run=false`.
  - Evidence from logs: dry-run offer built (`offer1...`, length `1056`), live Dexie post succeeded (`publish_failures=0`), returned offer id `TiwFMao5DuDDoyLQzi5PSSqLMY7bweRGcNDwKzRM8xy`, reconcile command executed without errors.
- Follow-up planning update:
  - Marked `docs/plan.md` G1 and G3 as complete based on repeated successful `dry_run=false` evidence on current native branch head (`run_id=22325031449`, `run_id=22325053517`).
  - Kept G2 open as the highest-priority remaining gap.

## 2026-02-23 (native-sdk migration prep)

- Implemented regression guardrails for moving off the forked `chia-wallet-sdk` bindings:
  - Added `tests/test_greenfloor_native_contract.py` to pin manager/signing behavior for native offer validation and offer spend-bundle construction contracts.
  - Added optional `tests/test_greenfloor_native_integration.py` (gated by `GREENFLOOR_RUN_NATIVE_INTEGRATION_TESTS=1`) to exercise real compiled native bindings.
- Added in-repo Rust extension crate `greenfloor-native/` (maturin+pyo3) exposing:
  - `validate_offer(offer_text)` (decode + driver parse validation),
  - `from_input_spend_bundle_xch(spend_bundle_bytes, requested_payments_xch)` (bytes-in/bytes-out constructor path).
- Switched Python call paths to use in-repo native bindings:
  - `greenfloor/signing.py` now constructs requested-payment tuples and calls `greenfloor_native.from_input_spend_bundle_xch`, then rebuilds `sdk.SpendBundle` from returned bytes.
  - `greenfloor/cli/manager.py` now prefers `greenfloor_native.validate_offer` before SDK-level fallback checks.
- Updated CI/workflows for native build path:
  - `.github/workflows/ci.yml` now installs Rust on all matrix runners, builds/installs `greenfloor-native`, and adds an Ubuntu native integration test step.
  - `.github/workflows/live-testnet-e2e.yml` now builds and installs `chia-wallet-sdk` from the pinned in-repo submodule wheel and builds `greenfloor-native` in-repo (no forked SDK wheel path).
- Repointed submodule metadata back to upstream:
  - `.gitmodules` now references `git@github.com:xch-dev/chia-wallet-sdk.git`.
  - Submodule pointer moved from fork commit `5a87495f` to upstream baseline `b3158279`.
- Validation status:
  - `cargo check --manifest-path greenfloor-native/Cargo.toml` passes (with `clvmr` pinned to `0.16.2` for dependency compatibility).
  - `pytest` passes locally (`151 passed, 5 skipped`), `ruff check` and `ruff format --check` pass, and `pyright` passes.
  - Local network SSL trust prevented PyPI install for running native integration tests in this environment (`SSLCertVerificationError`); CI path is in place to execute them.

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

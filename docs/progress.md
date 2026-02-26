# Progress Log

## 2026-02-26 (Coinset websocket default-path correction for daemon runtime)

- Root cause for repeated websocket `404` disconnects (`https://www.coinset.org/ws`) identified:
  - `greenfloor/config/models.py` populated `tx_block_websocket_url` with legacy mainnet default `wss://coinset.org/ws` when config left `websocket_url` blank.
  - this model-level default overrode daemon CLI base-URL defaults and routed websocket attempts to a non-API host.
- Implemented default-path correction:
  - `greenfloor/config/models.py` now defaults mainnet websocket URL to `wss://api.coinset.org/ws`.
  - `greenfloor/daemon/main.py` fallback websocket URL paths also use `wss://api.coinset.org/ws`.
  - `config/program.yaml` inline websocket default comment updated to match.
- Validation snapshot:
  - `.venv/bin/python -m pytest tests/test_config_load.py tests/test_daemon_websocket_runtime.py` -> `11 passed`.
- John-Deere rollout:
  - synced `greenfloor/config/models.py` and `greenfloor/daemon/main.py`,
  - restarted daemon,
  - verified effective runtime resolution now reports:
    - `program.tx_block_websocket_url = wss://api.coinset.org/ws`
    - `_resolve_coinset_ws_url(...) = wss://api.coinset.org/ws`.

## 2026-02-26 (websocket SQLite thread-safety fix + Coinset API default correction)

- Fixed daemon websocket callback SQLite thread-safety in `greenfloor/daemon/main.py`:
  - removed cross-thread reuse of one `SqliteStore` connection in `_run_loop` websocket callbacks.
  - websocket callbacks now open/close callback-local `SqliteStore` connections before writing mempool/confirm/audit events.
  - this removes the runtime crash seen on John-Deere:
    - `sqlite3.ProgrammingError: SQLite objects created in a thread can only be used in that same thread`.
- Corrected mainnet Coinset API defaults to the API host:
  - `greenfloor/adapters/coinset.py`: `CoinsetAdapter.MAINNET_BASE_URL` -> `https://api.coinset.org`.
  - `greenfloor/daemon/main.py`: CLI default `--coinset-base-url` -> `https://api.coinset.org`.
- Added deterministic regression coverage in `tests/test_daemon_websocket_runtime.py`:
  - new test asserts websocket callbacks can run on a worker thread without cross-thread store usage.
- Validation snapshot:
  - `.venv/bin/python -m pytest tests/test_daemon_websocket_runtime.py tests/test_coinset_adapter.py` -> `15 passed`.
- John-Deere deployment verification:
  - synced patched files to `/home/hoffmang/greenfloor/greenfloor/{daemon/main.py,adapters/coinset.py}`,
  - confirmed default adapter base URL resolves to `https://api.coinset.org`,
  - restarted `greenfloord` without Coinset env override and verified fee-estimate API call succeeds using defaults.

## 2026-02-26 (John-Deere mainnet cutover checklist execution for `carbon22_sell_wusdbc`)

- Updated repo baseline `config/program.yaml` to mainnet defaults and synced it to John-Deere repo path:
  - `app.network: mainnet`
  - `keys.registry[*].network: mainnet`
  - `venues.dexie.api_base: https://api.dexie.space`
- Confirmed John-Deere runtime home config already had mainnet + Cloud Wallet credentials populated (`~/.greenfloor/config/program.yaml`).
- Ran remote preflight successfully:
  - `config-validate` -> `config validation ok`
  - `doctor` -> `"ok": true` (warnings only for optional Pushover env vars).
- Executed market shaping checklist commands for `carbon22_sell_wusdbc` (`size_base_units` 1, 10, 100) with `--until-ready`:
  - all three returned `stop_reason: "ready"` and readiness targets met.
  - operational requirement discovered: `GREENFLOOR_COINSET_BASE_URL=https://api.coinset.org` needed on John-Deere; default `https://coinset.org` caused fee-preflight and coin-record 404s.
- Ran pre-daemon posting proof sequence successfully:
  - `build-and-post-offer --market-id carbon22_sell_wusdbc --size-base-units 1` posted to Dexie mainnet (`offer_id: 9xwe1eFzaKDVfuxkwhndzaYTepCtJwgCFJpFTcL8Jj8R`, `publish_failures: 0`).
  - `offers-status` showed persisted offer row and `strategy_offer_execution` evidence.
  - `offers-reconcile` completed and transitioned state to `mempool_observed` (Dexie fallback signal path).
- Started long-running daemon on John-Deere with Coinset override and ran canary status/reconcile checks.
- New blocker identified for continuous websocket signal ingestion:
  - `~/.greenfloor/logs/daemon-cutover.log` shows websocket thread crash:
    - `sqlite3.ProgrammingError: SQLite objects created in a thread can only be used in that same thread`
  - Trace points to websocket audit callback path (`greenfloor/daemon/coinset_ws.py` -> `greenfloor/daemon/main.py` -> `greenfloor/storage/sqlite.py`).
  - Current canary shows daemon cycle events continue, but websocket audit emission path is not thread-safe and needs remediation before declaring strict-close continuous-posting hardening complete.

## 2026-02-26 (mainnet continuous-posting cutover checklist implementation)

- Implemented an operator-ready cutover checklist in `docs/runbook.md` for promoting `carbon22_sell_wusdbc` from one-off manager proofs to continuous daemon posting.
- Added explicit step-by-step commands for:
  - mainnet runtime lock-in (`app.network`, `runtime.dry_run`, Dexie mainnet API, Cloud Wallet credentials),
  - canary market isolation (`carbon22_sell_wusdbc` only),
  - denomination shaping to target ladder buckets (`1:10`, `10:2`, `100:1`),
  - pre-daemon single-cycle validation (`build-and-post-offer` -> `offers-status` -> `offers-reconcile`),
  - long-running daemon startup (`greenfloord` without `--once`),
  - periodic canary verification loop commands scoped by market id.
- Added explicit canary pass criteria in runbook:
  - repeated successful `strategy_offer_execution` events,
  - maintained open-offer presence with only brief rollover gaps,
  - no persistent post failures across consecutive daemon cycles,
  - healthy websocket signal ingestion (`coinset_ws_*` without prolonged disconnect loops).

## 2026-02-26 (CI pre-commit cache stabilization + pytest runtime speedup)

- Fixed CI pre-commit cache path behavior in `.github/workflows/ci.yml`:
  - switched to a single `actions/cache@v4` step for restore/save lifecycle,
  - cache path now uses workspace-local `./.cache/pre-commit`,
  - `PRE_COMMIT_HOME` now uses absolute workspace path (`${{ github.workspace }}/.cache/pre-commit`),
  - added restore-key prefix fallback by `{os, arch, py311}` tuple.
- Verified cache health on subsequent CI run:
  - no path-validation warnings,
  - pre-commit cache archives successfully saved for all matrix targets (`Linux/X64`, `Linux/ARM64`, `macOS/ARM64`),
  - first run remained expected cold-start miss; next runs can hit saved keys.
- Removed a 30-second wall-clock delay from daemon runtime test coverage:
  - `tests/test_daemon_websocket_runtime.py::test_run_loop_refreshes_log_level_without_restart` now stubs `time.sleep` in loop mode,
  - preserved existing behavior assertions while eliminating real interval wait in deterministic tests.
- Validation snapshot after test-speed update:
  - targeted test: `1 passed in 0.13s`,
  - full suite: `278 passed, 3 skipped in 0.58s`.

## 2026-02-26 (post-output UX + market config normalization closeout)

- Improved operator UX for `build-and-post-offer` Dexie publishes:
  - Manager output now includes `results[].result.offer_view_url` whenever Dexie returns an offer ID.
  - URL is normalized from API base to browser host:
    - mainnet: `https://dexie.space/offers/<id>`
    - testnet: `https://testnet.dexie.space/offers/<id>`
  - Added deterministic coverage for standard + Cloud Wallet posting paths in `tests/test_manager_post_offer.py`.
- Finalized mainnet market config normalization:
  - Market IDs now follow pair/mode naming style (`carbon22_sell_xch`, `carbon22_sell_wusdbc`, `byc_two_sided_wusdbc`).
  - Mainnet receive addresses in base `config/markets.yaml` aligned to `xch1hpppalrmxk7x2vzvf5f5c4ylz6l9kwnjkanqtk3qszegrtkm2lvsr6h0df`.
  - Updated `carbon22_sell_wusdbc` bucket targets to `1:10`, `10:2`, `100:1`.
- Confirmed remote environment alignment on `John-Deere`:
  - branch fast-forwarded to latest commits,
  - runtime config files synced for `markets.yaml`, `cats.yaml`, and optional `testnet-markets.yaml`,
  - manager `config-validate` + CAT catalog smoke commands passed.

## 2026-02-26 (mainnet market config normalization + John-Deere sync)

- Normalized active mainnet market IDs in `config/markets.yaml` to pair-first naming:
  - `carbon22_sell_xch`
  - `carbon22_sell_wusdbc`
  - `byc_two_sided_wusdbc`
- Updated mainnet `receive_address` values in base `markets.yaml` to:
  - `xch1hpppalrmxk7x2vzvf5f5c4ylz6l9kwnjkanqtk3qszegrtkm2lvsr6h0df`
- Updated `carbon22_sell_wusdbc` inventory bucket targets to:
  - `1: 10`, `10: 2`, `100: 1`
- Added strict base-config address guard in `greenfloor/config/io.py`:
  - base `markets.yaml` now logs an error and fails validation when any market uses `txch1...` receive addresses,
  - directs testnet address usage to `testnet-markets.yaml` only.
- Synced remote `John-Deere` repo/config to current branch and config model:
  - pulled latest branch,
  - aligned `~/.greenfloor/config/{markets.yaml,cats.yaml,testnet-markets.yaml}` with repo,
  - revalidated manager config load path and CAT catalog smoke commands.

## 2026-02-26 (optional testnet markets overlay split from mainnet markets config)

- Split testnet market stanzas out of `config/markets.yaml` into new `config/testnet-markets.yaml`.
- Added optional markets-overlay loading in config layer:
  - `greenfloor.config.io.load_markets_config_with_optional_overlay(path=..., overlay_path=...)`
  - Manager and daemon now load overlay markets only when `--testnet-markets-config` is set (or auto-detected at `~/.greenfloor/config/testnet-markets.yaml`).
- Added manager/daemon global CLI support for optional testnet market config:
  - `greenfloor-manager --testnet-markets-config <path> ...`
  - `greenfloord --testnet-markets-config <path> ...`
- Added optional bootstrap seeding for developer testnet config:
  - `greenfloor-manager bootstrap-home --seed-testnet-markets` (from `config/testnet-markets.yaml`).
- Added deterministic tests:
  - `tests/test_config_load.py` overlay merge test.
  - `tests/test_home_bootstrap.py` optional testnet markets bootstrap seeding test.
- Updated runbook docs for optional testnet-markets overlay usage and seeding.

## 2026-02-26 (CAT catalog migration to config/cats.yaml + manager add/list commands)

- Moved CAT metadata catalog out of `config/markets.yaml` into dedicated `config/cats.yaml` (`cats:` list with `name`, `base_symbol`, `asset_id`, optional legacy price, and Dexie metadata fields).
- Added manager adjunct commands for CAT catalog operations:
  - `cats-list`: prints all known CATs from `--cats-config` (default `~/.greenfloor/config/cats.yaml`, fallback repo `config/cats.yaml`).
  - `cats-add`: adds or replaces CAT entries by `--cat-id` or `--ticker`, with Dexie-assisted lookup by default and full manual override fields (`--name`, `--base-symbol`, `--ticker-id`, `--pool-id`, `--last-price-xch`, `--target-usd-per-unit`).
- Added `--cats-config` global manager flag and bootstrap seeding support:
  - `bootstrap-home` now seeds `cats.yaml` via `--cats-template` (default `config/cats.yaml`) alongside `program.yaml` and `markets.yaml`.
- Updated local CAT label hint resolution fallback used by Cloud Wallet asset resolution:
  - Manager now reads hints from `config/cats.yaml` first, while retaining legacy market-based fallback behavior for compatibility.
- Added deterministic tests for CAT catalog command behavior and bootstrap seeding:
  - `tests/test_manager_cats.py` (manual add, Dexie-assisted add, replace guardrail).
  - `tests/test_home_bootstrap.py` now validates `cats.yaml` seed/create/keep behavior.

## 2026-02-26 (offer status/reconcile hardening: Dexie shape + Cloud Wallet filters + split-input hints)

- Fixed `offers-reconcile` Dexie status parsing to handle both response shapes:
  - top-level `status`,
  - nested `offer.status` (current live `/v1/offers/{id}` shape).
- Hardened Coinset tx-id extraction in `greenfloor/adapters/coinset.py`:
  - `extract_coinset_tx_ids_from_offer_payload` now recursively walks nested dict/list payloads instead of only top-level keys.
  - This allows reconciliation to recover tx-id evidence from nested venue payloads.
- Tightened Cloud Wallet offer artifact polling in `greenfloor/cli/manager.py`:
  - wallet offer polling now requests creator-owned active offers (`is_creator=True`, `states=["OPEN","PENDING"]`, bounded page size) before selecting new artifacts.
  - Added backward-compatible fallback for legacy test doubles that expose `get_wallet()` without filter args.
- Extended Cloud Wallet offer creation contract in `greenfloor/adapters/cloud_wallet.py`:
  - `create_offer` now passes `splitInputCoins` and `splitInputCoinsFee` through GraphQL input.
  - Manager Cloud Wallet posting path now supplies split-input options explicitly.
- Deterministic regression coverage added/updated:
  - `tests/test_manager_offer_reconcile.py`: nested Dexie payload status handling.
  - `tests/test_cloud_wallet_adapter.py`: create-offer split-input options + wallet-offer filter arguments.
  - `tests/test_manager_post_offer.py`: artifact polling filter arguments and updated Cloud Wallet create-offer fake signatures.
- Validation snapshot:
  - `.venv/bin/python -m pytest tests/test_manager_offer_reconcile.py tests/test_cloud_wallet_adapter.py tests/test_manager_post_offer.py -k "nested_dexie_offer_payload_shape or cloud_wallet_create_offer_includes_split_input_coin_options or cloud_wallet_get_wallet_passes_offer_filters or poll_offer_artifact_until_available_requests_creator_open_pending or build_and_post_offer_cloud_wallet"` -> `8 passed`

## 2026-02-26 (plan/progress clarification: dual Cloud Wallet cancel-mode support)

- Clarified active cancellation policy in planning/progress docs:
  - GreenFloor supports both Cloud Wallet cancellation modes:
    - standard on-chain cancellation (`cancelOffChain: false`),
    - off-chain cancellation (`cancelOffChain: true`) when org feature flag `OFFER_CANCEL_OFF_CHAIN` is enabled.
  - Until production support for `OFFER_CANCEL_OFF_CHAIN` is available, operational workflows proceed using the standard on-chain cancellation API.
- Updated `docs/plan.md` to document this dual-mode compatibility contract and explicit production-default behavior.

## 2026-02-26 (logging stream alignment + runtime log-level controls + CI drift hardening)

- Merged logging alignment and operator controls via PR `#37`:
  - Added shared rotating file logging setup in `greenfloor/logging_setup.py` using `ConcurrentRotatingFileHandler`.
  - Manager and daemon now log to `~/.greenfloor/logs/debug.log` with rotation policy:
    - `maxBytes=25 MiB`
    - `backupCount=4`
  - Signed-offer artifact logs moved from stderr prints to structured INFO logs on the rotating file stream.
- Added runtime-configurable log levels from `app.log_level` in `config/program.yaml`:
  - Program config parsing now normalizes/validates level values (`CRITICAL|ERROR|WARNING|INFO|DEBUG|NOTSET`).
  - Missing `app.log_level` is auto-healed to `INFO` in `program.yaml`.
  - Added warning diagnostics when auto-heal occurs after logging initialization.
  - Added manager command `set-log-level --log-level <LEVEL>` to update `program.yaml` safely.
- Added daemon runtime log-level refresh without restart:
  - daemon loop now reapplies configured log level each cycle so operator updates take effect live.
- Added websocket failure diagnostics and guardrails:
  - WARN-level websocket disconnect/recovery failure logs in `greenfloor/daemon/coinset_ws.py`.
  - Added `NullHandler` on websocket module logger to avoid noisy fallback behavior outside daemon bootstrap.
- CI/local tooling drift hardening:
  - CI now runs `pre-commit run --all-files` from the project venv environment.
  - Pinned dev tooling versions in `pyproject.toml` to match local pre-commit execution:
    - `ruff==0.9.10`
    - `pyright==1.1.408`
    - `pytest==9.0.2`
    - `pre-commit==4.5.1`
  - Updated local pre-commit hook entries for pyright/pytest to call `.venv` binaries directly.
- Added deterministic test coverage for:
  - log-level defaulting/auto-heal,
  - manager log-level CLI dispatch/update,
  - daemon runtime log-level refresh,
  - daemon startup/shutdown and auto-heal warning log emission,
  - websocket WARN logging behavior.

## 2026-02-26 (course pivot: defer OFFER_CANCEL_OFF_CHAIN work; add signed-offer logging)

- Course adjustment recorded:
  - Off-chain cancel follow-up validation is deferred until Cloud Wallet org feature flag `OFFER_CANCEL_OFF_CHAIN` is restored.
  - Current branch keeps notes and implementation context so work can resume quickly once the flag is available.
- Added high-signal Cloud Wallet offer logging in the standard manager offer flow (`greenfloor/cli/manager.py`):
  - When a signed offer artifact (`offer1...`) is retrieved after signature submission, manager now logs:
    - full offer file text on one line (`signed_offer_file:...`),
    - then a metadata line with `ticker`, `coinid`, `amount`, `trading_pair`, and `expiry`.
- Added deterministic test coverage in `tests/test_manager_post_offer.py` to assert both log lines are emitted in the Cloud Wallet happy-path posting flow.

## 2026-02-26 (off-chain cancel follow-up with Cloud Wallet on-chain refresh request)

- Implemented manager follow-up flow for Cloud Wallet cancellations in `greenfloor/cli/manager.py`:
  - `offers-cancel` now supports optional flags:
    - `--submit-onchain-after-offchain`
    - `--onchain-market-id` / `--onchain-pair` (market context for refresh coin selection)
  - When follow-up mode is enabled and the cancel succeeds, manager now:
    - resolves the market asset in Cloud Wallet,
    - selects a spendable coin (preferring coin-id hints derived from offer spend contents),
    - resolves fee via the coin-op standard (`_resolve_taker_or_coin_operation_fee`),
    - submits Cloud Wallet `splitCoins` (`numberOfCoins=1`, full amount) to produce an on-chain coin-name refresh request.
  - Command output now includes structured `onchain_refresh` metadata with `signature_request_id`, `signature_state`, selected coin details, and fee source.
- Added deterministic coverage in `tests/test_manager_post_offer.py` for:
  - successful follow-up refresh request submission path,
  - market-selection guardrail for follow-up mode.
- Remote validation on `John-Deere` confirmed execution wiring and identified current blocker:
  - off-chain cancellation attempt failed with `Organization does not have required feature flags: OFFER_CANCEL_OFF_CHAIN`.
  - this blocks collecting a live `onchain_refresh.signature_request_id` until the org flag is enabled.
- Governance/update note:
  - Updated `AGENTS.md` PR gate policy so required PR checks now use `pre-commit run --all-files` as the single required check.
- Branch/PR status:
  - Branch: `feat/cloud-wallet-onchain-refresh-followup`
  - Draft PR: `#36` (`WIP: Cloud Wallet off-chain cancel with on-chain refresh follow-up`)

## 2026-02-26 (remote CARBON22 split + live offer proof; resolver hardening)

- Completed remote Cloud Wallet execution on host `John-Deere` for current mainnet `CARBON22:wUSDC.b` workstream:
  - Confirmed `CARBON22` wallet asset (`Asset_ymgm3ygl5om7ia4u9llk3iu7`) recovered to spendable state after self-transfer (`totalAmount: 200000`, `spendableAmount: 200000` at verification time).
  - Submitted `coin-split` for `carbon_2022_wusdc_sell` with `amount_per_coin=10000` and `number_of_coins=10`; split request id `SignatureRequest_az9aoi4gxqlur4ccbfpsice8` moved to `SUBMITTED` and then into pending output state.
  - Posted a live test offer after split using current spendable chunk:
    - command: `build-and-post-offer --market-id carbon_2022_wusdc_sell --size-base-units 10`
    - result: success; offer id `8UjTyuLpooC7GAwrTzni6QK13p6yQPTZaVmjRFtZssVk`
    - signature request id `SignatureRequest_g3s0vutpbq25polq8am6ork9`, state `SUBMITTED`
    - resolver output confirmed canonical mapping: base `Asset_ymgm3ygl5om7ia4u9llk3iu7` (CARBON22), quote `Asset_cxc7mql006dp2w3kigqlj58t` (wUSDC.b)
  - `offers-status --market-id carbon_2022_wusdc_sell` now reports the new offer in `open` state with fresh `strategy_offer_execution` evidence.
- Closed remote resolver drift for `coins-list --asset <CAT-hex>`:
  - Root cause: CAT-hex resolution previously hard-failed when Dexie metadata for the CAT was missing/stale.
  - Fix in `greenfloor/cli/manager.py`: added deterministic local catalog hint fallback (`config/markets.yaml` assets + markets `base_symbol`) and direct wallet-label matching while preserving strict ambiguity rejection.
  - Verified remotely: `coins-list --asset 4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7` now resolves and lists `Asset_ymgm...` coins instead of raising `cloud_wallet_asset_resolution_failed:unmatched_wallet_cat_asset_for`.

## 2026-02-25 (mainnet target alignment: CARBON22:wUSDC.b strict-close)

- Updated planning alignment for live target execution:
  - `docs/plan.md` active target now treats `CARBON22:wUSDC.b` as the primary mainnet strict-close objective, with `CARBON22:xch` retained as completed supporting proof.
  - Corrected stale historical wording that implied a 7-command current CLI core surface; current v1 core surface remains 10 commands.
- Declared next execution evidence path:
  - Run one end-to-end manager proof on mainnet market `carbon_2022_wusdc_sell` (`build-and-post-offer` -> `offers-status` -> `offers-reconcile`) and record persisted lifecycle output.
- Operational preflight reminder for this proof:
  - Ensure mainnet receive address format (`xch1...`) and mainnet endpoint routing are in place before execution.
- Remote execution attempt on host `John-Deere`:
  - Fixed remote preflight blocker in `~/.greenfloor/config/program.yaml` (`chain_signals.tx_block_trigger.mode` updated from `webhook_or_poll` to `websocket`) so `config-validate` and `doctor` could run.
  - `config-validate` now passes and `doctor` reports `ok: true` (warnings only for missing optional Pushover env vars).
  - Updated remote `carbon_2022_wusdc_sell` market stanza to use canonical `wUSDC.b` CAT tail (`fa4a...a99d`) and mainnet `xch1...` receive address.
  - `build-and-post-offer --market-id carbon_2022_wusdc_sell --size-base-units 1 --dry-run` failed with `offer_builder_failed:signing_failed:missing_mnemonic_for_key_id` (dry-run uses local signing path, no mnemonic in shell env).
  - Live `build-and-post-offer` failed before publish with `cloud_wallet_asset_resolution_failed:dexie_cat_metadata_not_found_for:4a1689...66e7`.
  - `coins-list` on the target vault currently returns only `Asset_huun64oh7dbt9f1f9ie8khuw` coins, so no clear in-vault `CARBON22`/`wUSDC.b` inventory signal is visible yet.

## 2026-02-25 (upstreaming follow-up: cache key + metadata + formatting)

- Closed CI follow-up issues discovered after merging reliability test hardening:
  - Fixed persistent `greenfloor-native` wheel cache misses in `.github/workflows/ci.yml` by replacing `hashFiles('greenfloor-native/**')` cache keys (which changed after build artifacts were produced) with a stable git tree hash resolved via `git rev-parse HEAD:greenfloor-native`.
  - Added explicit `project.version = "0.1.0"` to `greenfloor-native/pyproject.toml` to satisfy PEP 621 metadata requirements and remove wheel-build warning noise.
  - Resolved CI formatting parity issue by applying repository `prettier` output for `config/program.yaml` inline comment spacing.
- Validation snapshot:
  - `PATH="/Users/hoffmang/src/greenfloor/.venv/bin:$PATH" .venv/bin/pre-commit run --all-files` -> all hooks passed (`ruff`, `ruff-format`, `prettier`, `yamllint`, `pyright`, `pytest`).
- Upstreaming intent:
  - prepared `feat/step4-monitoring-reliability` for PR into `main` with these follow-up reliability/CI fixes included.

## 2026-02-25 (testing hardening: reliability + adapter contracts)

- Added broad deterministic testing hardening focused on runtime reliability boundaries and error-contract stability:
  - Added `tests/test_cloud_wallet_adapter.py` with direct `CloudWalletAdapter` coverage for GraphQL pagination, response-shape validation, HTTP/network error classification, and signature-request fallback behavior.
  - Expanded `tests/test_manager_offer_reconcile.py` with a Coinset-first reconciliation matrix covering confirmed vs mempool vs no-signal states, missing-status behavior, and Dexie-status fallback transitions.
  - Expanded `tests/test_coinset_ws.py` for websocket robustness paths: parse errors, ignored non-dict payloads, recovery-poll success/error audit emissions, and stop-aware sleep behavior.
  - Expanded venue adapter test contracts:
    - `tests/test_dexie_adapter.py`: HTTP/network failures, invalid response formats, and input validation.
    - `tests/test_splash_adapter.py`: invalid response formats and HTTP-error propagation contract.
  - Expanded signing/store regression hardening:
    - `tests/test_signing.py`: AGG_SIG domain/unsafe parsing and broadcast failure/success contracts.
    - `tests/test_sqlite_store.py`: `get_tx_signal_state` dedupe/normalization behavior and audit-event filter/limit contracts.
- Validation snapshots:
  - `.venv/bin/python -m pytest tests/test_cloud_wallet_adapter.py tests/test_manager_offer_reconcile.py` -> `11 passed`
  - `.venv/bin/python -m pytest tests/test_coinset_ws.py tests/test_dexie_adapter.py tests/test_splash_adapter.py` -> `17 passed`
  - `.venv/bin/python -m pytest tests/test_signing.py tests/test_signing_cat_parse_regression.py` -> `31 passed`
  - `.venv/bin/python -m pytest tests/test_sqlite_store.py tests/test_tx_signal_state.py` -> `9 passed`
  - `PATH="/Users/hoffmang/src/greenfloor/.venv/bin:$PATH" .venv/bin/pre-commit run --all-files` -> all hooks passed (`ruff`, `ruff-format`, `prettier`, `yamllint`, `pyright`, `pytest`)
- Branch/PR status:
  - Branch `test/enhance-reliability-coverage` pushed to origin.
  - Opened PR `#30` (`test/enhance-reliability-coverage` -> `feat/step4-monitoring-reliability`).

## 2026-02-25 (coinset websocket-only daemon signal ingestion)

- Migrated daemon signal ingestion from webhook-startup + cycle polling to websocket-only runtime:
  - Added `greenfloor/daemon/coinset_ws.py` with long-lived Coinset websocket client behavior (reconnect loop, payload normalization, tx-id routing, and recovery-poll hooks).
  - `greenfloor/daemon/main.py` now starts/stops the websocket client in `_run_loop` and writes tx signals to `tx_signal_state` through the same SQLite persistence paths used by reconciliation.
  - Removed daemon webhook server startup from active runtime path; websocket is the primary tx signal source.
- Added bounded websocket capture for `greenfloord --once`:
  - `run_once` now supports websocket capture mode with recovery snapshot before continuing through normal cycle execution.
- Extended config model/defaults for websocket controls:
  - `chain_signals.tx_block_trigger.mode` now enforces `websocket` in parser,
  - added `websocket_url`, `websocket_reconnect_interval_seconds`, and `fallback_poll_interval_seconds` handling.
- Added deterministic tests:
  - `tests/test_coinset_ws.py` for payload classification/callback routing.
  - `tests/test_daemon_websocket_runtime.py` for websocket client startup/shutdown wiring and `run_once` websocket capture path.
  - Updated `tests/test_config_load.py` and `tests/test_low_inventory_alerts.py` for new `ProgramConfig` fields.
- Validation snapshot:
  - `.venv/bin/python -m pytest tests/test_config_load.py tests/test_coinset_ws.py tests/test_daemon_websocket_runtime.py tests/test_manager_offer_reconcile.py tests/test_daemon_multi_cycle_integration.py tests/test_low_inventory_alerts.py` -> `13 passed`

## 2026-02-25 (coinset webhook-first offer-taken reconciliation)

- Refactored offer lifecycle/taker detection to prefer Coinset tx signals (webhook + mempool state) over Dexie status heuristics:
  - `greenfloor/cli/manager.py` `offers-reconcile` now extracts tx ids from venue payloads and checks `tx_signal_state` first.
  - Canonical taker signal now emits from Coinset confirmation evidence (`taker_signal: "coinset_tx_block_webhook"`), with Dexie status retained as fallback diagnostics only.
  - Reconcile/audit payloads now include signal-source metadata and Coinset tx-id evidence (`signal_source`, `coinset_*_tx_ids`).
- Extended daemon lifecycle reconciliation to apply the same Coinset-first transition policy:
  - `greenfloor/daemon/main.py` now derives `offer_lifecycle_transition` primarily from Coinset tx signal state when tx ids are available, then falls back to Dexie status mapping.
- Added storage support for tx-signal lookups by tx id:
  - `greenfloor/storage/sqlite.py` now exposes `get_tx_signal_state(tx_ids)` for deterministic Coinset-backed offer-state transitions.
- Simplification follow-up:
  - Centralized tx-id extraction helpers into `greenfloor/adapters/coinset.py` (`extract_coinset_tx_ids_from_offer_payload`) so manager and daemon use one shared implementation.
- Added/updated deterministic tests:
  - `tests/test_manager_offer_reconcile.py` now asserts Coinset webhook-based taker signal emission.
  - `tests/test_daemon_multi_cycle_integration.py` now seeds tx-signal confirmation and asserts daemon lifecycle events mark `signal_source: "coinset_webhook"`.
- Validation snapshot:
  - `.venv/bin/python -m pytest tests/test_manager_offer_reconcile.py tests/test_daemon_multi_cycle_integration.py` -> `3 passed`
  - `PATH="/Users/hoffmang/src/greenfloor/.venv/bin:$PATH" .venv/bin/pre-commit run --all-files` -> all hooks passed (`ruff`, `ruff-format`, `prettier`, `yamllint`, `pyright`, `pytest`)

## 2026-02-25 (H1 coinset fee preflight diagnostics closure)

- Closed plan item H1 in `greenfloor/cli/manager.py` for coin-op fee lookup hardening:
  - Added deterministic Coinset fee preflight (`_coinset_fee_lookup_preflight`) before taker/coin-op fee resolution.
  - Preflight validates endpoint/network routing and usable fee-advice response before `coin-split` / `coin-combine` submission.
  - Added explicit failure classification via structured error contracts:
    - `coinset_fee_preflight_failed:endpoint_validation_failed`
    - `coinset_fee_preflight_failed:temporary_fee_advice_unavailable`
  - Failure payloads now include `coinset_fee_lookup` diagnostics (`coinset_base_url`, `coinset_network`, failure detail).
- Added deterministic tests in `tests/test_manager_post_offer.py`:
  - resolver preflight failure classification tests,
  - coin-op JSON failure contract tests for both endpoint-validation and temporary-advice-unavailable paths.
- Updated operator docs:
  - `docs/runbook.md` now documents fee-preflight behavior, endpoint override/debug steps, and expected JSON failure contracts.
  - `docs/plan.md` marks H1 complete.
- Validation snapshot:
  - `.venv/bin/python -m pytest tests/test_manager_post_offer.py` -> `79 passed`
  - `PATH="/Users/hoffmang/src/greenfloor/.venv/bin:$PATH" .venv/bin/pre-commit run --all-files` -> all hooks passed (`ruff`, `ruff-format`, `prettier`, `yamllint`, `pyright`, `pytest`)

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
  - `offers-reconcile` now emits `taker_signal` and `taker_diagnostic` fields; this initial Dexie-pattern implementation was later superseded by the `2026-02-25 (coinset webhook-first offer-taken reconciliation)` update at the top of this log.
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

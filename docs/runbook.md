# GreenFloor Operator Runbook

This runbook covers first deployment and recovery workflows for GreenFloor v1.

## 1) First Deployment (Clean Machine)

1. Install dependencies:
   - `python -m pip install -e ".[dev]"`
2. Bootstrap runtime home:
   - `greenfloor-manager bootstrap-home`
3. Validate seeded configs:
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml config-validate`
   - Optional testnet overlay (only if file exists): add `--testnet-markets-config ~/.greenfloor/config/testnet-markets.yaml`
   - Base `markets.yaml` must use mainnet `xch1...` receive addresses; `txch1...` addresses are rejected and belong in `testnet-markets.yaml`.
4. Onboard signer selection:
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml keys-onboard --key-id key-main-1 --state-dir ~/.greenfloor/state`
5. Run readiness checks:
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml doctor`
6. Run first daemon cycle:
   - `greenfloord --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml --state-dir ~/.greenfloor/state --once`
   - Optional testnet overlay (only if file exists): add `--testnet-markets-config ~/.greenfloor/config/testnet-markets.yaml`

Optional developer bootstrap for testnet markets:

- `greenfloor-manager bootstrap-home --seed-testnet-markets`
- This seeds `~/.greenfloor/config/testnet-markets.yaml` from `config/testnet-markets.yaml`.
- If you do not seed/use this file, runtime behavior remains mainnet-markets only.

## 2) Steady-State Operations

- Cloud Wallet config prerequisite (required for vault-first paths):
  - Set `cloud_wallet.base_url`, `cloud_wallet.user_key_id`, `cloud_wallet.private_key_pem_path`, and `cloud_wallet.vault_id` in `~/.greenfloor/config/program.yaml`.
  - Where to find each value:
    - `cloud_wallet.base_url`: open `https://vault.chia.net/settings.json`, read `GRAPHQL_URI`, and keep the origin only (for example `https://api.vault.chia.net`, not `/graphql`).
    - `cloud_wallet.user_key_id`: in Cloud Wallet UI, go to **Settings -> API Keys** (`/settings/api-keys`), create/select a key, copy **Key Id**.
    - `cloud_wallet.private_key_pem_path`: from the **API Key Created** modal, click **Download Key** and save the PEM file (recommended: `~/.greenfloor/keys/cloud-wallet-user-auth-key.pem`).
      The file must contain full PEM text (`-----BEGIN PRIVATE KEY-----` ... `-----END PRIVATE KEY-----`), not base64-only text.
    - `cloud_wallet.vault_id`: open the target vault and copy the URL segment in `.../wallet/<ID>/...`; use the `Wallet_...` value, not `vaultLauncherId`.
- Review vault coin inventory before shaping or posting:
  - `greenfloor-manager coins-list`
  - Optional asset scope: `greenfloor-manager coins-list --asset <ticker|CAT-id|Asset-id|xch>`
- Shape denominations for the selected market context:
  - Split: `greenfloor-manager coin-split --pair TDBX:txch --coin-id <coin-id> --amount-per-coin 1000 --number-of-coins 10`
  - Combine: `greenfloor-manager coin-combine --pair TDBX:txch --input-coin-count 10 --asset-id xch`
  - Config-driven shaping (from market `ladders.sell`): `greenfloor-manager coin-split --pair TDBX:txch --size-base-units 10`
  - Config-driven combine threshold (from market `ladders.sell`): `greenfloor-manager coin-combine --pair TDBX:txch --size-base-units 10`
  - Optional venue context annotation for prep commands: add `--venue dexie` or `--venue splash` (coin-prep works without it).
  - Optional readiness loop: add `--until-ready --max-iterations 3` to run bounded list/split-or-combine/wait/re-check cycles.
  - Coin-op submission now runs Coinset fee-lookup preflight first; commands fail fast when fee endpoint routing is invalid or conservative fee advice is temporarily unavailable.
  - `coin-split` with no `--coin-id` uses adapter-managed coin selection (`coin_selection_mode: "adapter_auto_select"` in JSON output).
  - When `--coin-id` is provided with `--until-ready`, loop retries may stop early with `stop_reason: "requires_new_coin_selection"` after the selected inputs are consumed.
  - Defaults wait through signature + mempool + confirmation; use `--no-wait` for async mode.
  - Wait mode now includes `reorg_watch_*` events after first confirmation: manager monitors six additional blocks before returning success.
  - Every `in_mempool` wait event includes a `coinset_url` and read-only Coinset reconciliation metadata (`confirmed_block_index`, `spent_block_index` when available).
  - Signature waits emit periodic `signature_wait_warning` and additive `signature_wait_escalation` events (soft-timeout behavior; manager continues waiting unless operator aborts the command).
- Post a real offer file directly (fast path to running state):
  - Mainnet (default, pair-based): `greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1`
  - Testnet (active proof pair): `greenfloor-manager build-and-post-offer --pair TDBX:txch --size-base-units 1 --network testnet11`
  - On `testnet11`, use `txch` in pair syntax.
  - Safe preflight (build only, no publish): `greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1 --dry-run`
  - If multiple markets share the same pair, rerun with explicit `--market-id`.
  - Use `--markets-config` only when overriding the default config path.
  - Use `--testnet-markets-config ~/.greenfloor/config/testnet-markets.yaml` only when you want to include optional testnet market stanzas.
  - Publish venue is selected by `venues.offer_publish.provider` in `~/.greenfloor/config/program.yaml` (`dexie` or `splash`).
    - Optional one-off override: `--venue dexie` or `--venue splash`
    - Optional URL overrides: `--dexie-base-url ...` and `--splash-base-url ...`
  - Dexie path validates offer text with `chia-wallet-sdk` before submission; if validation fails, manager blocks submit and returns a `wallet_sdk_offer_verify_*` error.
  - On successful Dexie post, command JSON now includes a direct browser link:
    - `results[].result.offer_view_url` (for example `https://dexie.space/offers/<offer_id>`).
- Reconcile posted offers and flag orphan/unknown entries:
  - `greenfloor-manager offers-reconcile --limit 200`
  - Optional scope: `--market-id <id>`
  - Reconcile output includes:
    - `taker_signal`: Coinset-confirmed taker signal (`none` or `coinset_tx_block_webhook`).
    - `taker_diagnostic`: advisory diagnostics (`coinset_tx_block_confirmed`, `coinset_mempool_observed`, or Dexie fallback patterns).
- View compact offer execution/reconciliation state:
  - `greenfloor-manager offers-status --limit 50 --events-limit 30`
- Note: manager CLI v1 core surface remains focused on trading/runtime commands. `offers-cancel`, `cats-add`, `cats-list`, and `cats-delete` are adjunct operator commands tracked outside the core-count policy. Tuning/history/metrics helpers are deferred until after G1-G3 testnet proof.

### Mainnet continuous-posting cutover (`carbon22_sell_wusdbc`)

Use this checklist when promoting from one-off manager proof runs to continuous daemon posting.

1. Lock runtime config to mainnet values:
   - `app.network: mainnet`
   - `runtime.dry_run: false`
   - `venues.dexie.api_base: "https://api.dexie.space"`
   - populated `cloud_wallet.base_url`, `cloud_wallet.user_key_id`, `cloud_wallet.private_key_pem_path`, `cloud_wallet.vault_id`
2. Isolate the canary market:
   - keep `carbon22_sell_wusdbc` enabled in `~/.greenfloor/config/markets.yaml`
   - temporarily disable unrelated markets during initial canary
3. Re-run preflight before go-live:
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml config-validate`
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml doctor`
4. Shape CARBON22 inventory to ladder targets (`1:10`, `10:2`, `100:1`):
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml coin-split --market-id carbon22_sell_wusdbc --size-base-units 1 --until-ready --max-iterations 5`
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml coin-split --market-id carbon22_sell_wusdbc --size-base-units 10 --until-ready --max-iterations 5`
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml coin-split --market-id carbon22_sell_wusdbc --size-base-units 100 --until-ready --max-iterations 3`
5. Confirm one manager posting cycle works before daemon handoff:
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml build-and-post-offer --market-id carbon22_sell_wusdbc --size-base-units 1`
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml offers-status --market-id carbon22_sell_wusdbc --limit 20 --events-limit 20`
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml offers-reconcile --market-id carbon22_sell_wusdbc --limit 50`
6. Start long-running daemon mode (no `--once`):
   - `greenfloord --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml --state-dir ~/.greenfloor/state`
7. Run canary verification loop every 2-5 minutes for at least 30 minutes:
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml offers-status --market-id carbon22_sell_wusdbc --limit 20 --events-limit 20`
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml offers-reconcile --market-id carbon22_sell_wusdbc --limit 50`
8. Canary pass criteria:
   - repeated successful `strategy_offer_execution` events for `carbon22_sell_wusdbc`
   - at least one open offer maintained except brief rollover windows
   - no persistent post failures across consecutive daemon cycles
   - websocket tx-signal events (`coinset_ws_*`) continue without prolonged disconnect loops

## 3) Recovery and Revalidation

- Re-seed home config from repo templates (if needed):
  - `greenfloor-manager bootstrap-home --force`
- Re-run deterministic preflight checks:
  - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml config-validate`
  - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml doctor`
- Re-check persisted offer state after incident:
  - `greenfloor-manager offers-status --limit 50 --events-limit 30`
  - `greenfloor-manager offers-reconcile --limit 200`

## 4) Expected Audit Signals

Monitor `audit_event` records in `~/.greenfloor/db/greenfloor.sqlite`:

- `xch_price_snapshot`: current price captured for strategy/cancel policy.
- `strategy_actions_planned`: deterministic action plan from strategy core.
- `strategy_offer_execution`: offer build/post execution results.
- `offer_cancel_policy`: cancel eligibility and triggered/non-triggered reasons.
- `offer_lifecycle_transition`: offer state transitions from Dexie status.
- `coin_ops_plan` and `coin_op_*`: split/combine planning and execution outcomes.
- `taker_detection`: canonical taker transition events produced by `offers-reconcile`.

## 5) Incident Triage

- **Price unavailable:** look for `xch_price_error`; XCH planning is price-gated and may produce no actions.
- **Offer builder failures:** check `strategy_offer_execution.items[].reason` for `offer_builder_*`.
- **Dexie post/cancel issues:** look for `dexie_offers_error`, `strategy_offer_execution` skip reasons, and `offer_cancel_policy` skip reasons.
- **Extended waits on coin operations:** inspect `wait_events` for `poll_retry`, `signature_wait_*`, `in_mempool`, `confirmed`, and `reorg_watch_*` events to determine whether delay is signer-side, mempool-side, Coinset API-side, or chain-depth-side.
- **Coin-op fee preflight failures:** inspect JSON `error` and `coinset_fee_lookup`:
  - `error: "coinset_fee_preflight_failed:endpoint_validation_failed"` means endpoint routing/configuration failure (invalid/misrouted `GREENFLOOR_COINSET_BASE_URL`, wrong-network endpoint, DNS/TLS/connectivity issues).
  - `error: "coinset_fee_preflight_failed:temporary_fee_advice_unavailable"` means Coinset endpoint is reachable but currently not returning usable fee advice.
  - `coinset_fee_lookup.coinset_base_url` + `coinset_fee_lookup.coinset_network` report exactly which endpoint/network pair was validated.
- **Websocket signal ingestion issues:** inspect daemon audit events `coinset_ws_*` (`coinset_ws_connecting`, `coinset_ws_connected`, `coinset_ws_disconnected`, `coinset_ws_recovery_poll*`) and validate `chain_signals.tx_block_trigger.websocket_url` + network endpoint routing.
- **Cancel policy not triggering:** verify market `quote_asset_type` is `unstable`, `pricing.cancel_policy_stable_vs_unstable: true`, and compare `move_bps` vs `threshold_bps` in `offer_cancel_policy`.

## 6) Runtime Controls

- Cancel threshold for unstable-leg movement:
  - `GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS` (default: `500`)
- Offer-post retry/cooldown controls:
  - `GREENFLOOR_OFFER_POST_MAX_ATTEMPTS` (default: `2`, min `1`)
  - `GREENFLOOR_OFFER_POST_BACKOFF_MS` (default: `250`, min `0`)
  - `GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS` (default: `30`, min `0`)
- Offer-cancel retry/cooldown controls:
  - `GREENFLOOR_OFFER_CANCEL_MAX_ATTEMPTS` (default: `2`, min `1`)
  - `GREENFLOOR_OFFER_CANCEL_BACKOFF_MS` (default: `250`, min `0`)
  - `GREENFLOOR_OFFER_CANCEL_COOLDOWN_SECONDS` (default: `30`, min `0`)
- Offer-builder override command:
  - `GREENFLOOR_OFFER_BUILDER_CMD`
- Coinset endpoint override (coin reads + chain history + tx submit):
  - `GREENFLOOR_COINSET_BASE_URL`
  - Default behavior: mainnet endpoint when unset; testnet11 endpoint when market/network is `testnet11`.
  - For `testnet11`, do not route to mainnet Coinset endpoint unless you explicitly set `GREENFLOOR_ALLOW_MAINNET_COINSET_FOR_TESTNET11=1` for temporary debugging.
- Daemon tx-signal ingestion controls (`~/.greenfloor/config/program.yaml` -> `chain_signals.tx_block_trigger`):
  - `mode`: must be `websocket`
  - `websocket_url`: Coinset websocket endpoint (defaults by network when blank)
  - `websocket_reconnect_interval_seconds`: reconnect cadence after disconnect/error (must be `>= 1`)
  - `fallback_poll_interval_seconds`: recovery snapshot window used by websocket reconnect and `greenfloord --once` bounded capture
- Strategy execution dry-run:
  - set `runtime.dry_run` in `~/.greenfloor/config/program.yaml`
- Validate config + override sanity before deploy:
  - `greenfloor-manager doctor` (includes warnings for invalid runtime override env values)

## 7) Golden Path Smoke Test

Run this sequence for first operator user testing:

1. `greenfloor-manager bootstrap-home`
2. `greenfloor-manager config-validate`
3. `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml doctor`
4. Replace placeholder `receive_address` values in `~/.greenfloor/config/markets.yaml` with a valid network address (`xch1...` on mainnet, `txch1...` on `testnet11`).
5. `greenfloor-manager build-and-post-offer --pair TDBX:txch --size-base-units 1 --network testnet11 --dry-run`
6. `greenfloor-manager build-and-post-offer --pair TDBX:txch --size-base-units 1 --network testnet11`
7. `greenfloor-manager offers-status --limit 50 --events-limit 30`
8. `greenfloor-manager offers-reconcile --limit 200`

## 8) Testnet11 Asset Bring-Up (On-Chain Testing)

Use this checklist to stage on-chain testing while keeping GreenFloor on `chia-wallet-sdk` paths.

Optional CI workflow secret contract (`.github/workflows/live-testnet-e2e.yml`):

- Configure `TESTNET_WALLET_MNEMONIC` as an importable mnemonic phrase.
- Expected format is plain whitespace-delimited `12` or `24` words.
- Current testnet receive address: `txch1t37dk4kxmptw9eceyjvxn55cfrh827yf5f0nnnm2t6r882nkl66qknnt9k`.
- For `greenfloor-manager`, global flags (like `--program-config` and `--markets-config`) must be passed before the command name.
- This live workflow does not run pytest/simulator harness steps; it runs manager/daemon proof commands and uploads their logs.
- Live workflow now supports manager-proof inputs: `network_profile`, `pair`, `size_base_units`, and `dry_run`.
- Workflow sets `GREENFLOOR_CHIA_KEYS_DERIVATION_SCAN_LIMIT=1000` by default to reduce missed funded keys at deeper derivation indices.
- Workflow uploads `live-testnet-e2e-artifacts` containing dry-run/live/status/reconcile/daemon logs.

G2 helper workflow for market bootstrap snippet generation (`.github/workflows/testnet11-asset-bootstrap-helper.yml`):

- Dispatch `Testnet11 Asset Bootstrap Helper (G2)` from Actions.
- Use defaults unless you need custom Dexie endpoint, signer key id, or receive address.
- Download `g2-testnet-asset-bootstrap-artifacts` and use:
  - `selected-assets.json` for candidate review,
  - `markets-snippet.yaml` for copy/paste bootstrap stanzas into `~/.greenfloor/config/markets.yaml`.

CI-only proof sequence (no local mnemonic required):

1. Open GitHub Actions and dispatch `Live Testnet E2E (Optional)`.
2. Use `network_profile=testnet11`, `pair=TDBX:txch`, `size_base_units=1`.
3. Set `dry_run=false` to execute full G1/G3 evidence path.
4. Download `live-testnet-e2e-artifacts` and confirm:
   - dry-run build succeeds,
   - live build/post returns an offer id,
   - `offers-status` shows posted lifecycle data,
   - `offers-reconcile` completes without hard errors.

5. Start with known testnet11 CAT assets that already trade on Dexie testnet.
6. Fund a testnet11 `TXCH` account (faucet) for fees and initial taker actions.
7. Acquire small test inventory in the target CAT by taking existing testnet offers.
8. Add those asset IDs to `~/.greenfloor/config/markets.yaml` as enabled test markets.
9. Run manager preflight and dry-run offer builds:
   - `greenfloor-manager config-validate`
   - `greenfloor-manager build-and-post-offer --pair <TESTCAT>:txch --size-base-units 1 --network testnet11 --dry-run`
10. Publish small-size offers and reconcile:
    - `greenfloor-manager build-and-post-offer --pair <TESTCAT>:txch --size-base-units 1 --network testnet11`
    - `greenfloor-manager offers-status --limit 50 --events-limit 30`
    - `greenfloor-manager offers-reconcile --limit 200`

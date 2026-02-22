# GreenFloor Operator Runbook

This runbook covers first deployment and recovery workflows for GreenFloor v1.

## 1) First Deployment (Clean Machine)

1. Install dependencies:
   - `python -m pip install -e ".[dev]"`
2. Bootstrap runtime home:
   - `greenfloor-manager bootstrap-home`
3. Validate seeded configs:
   - `greenfloor-manager config-validate --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml`
4. Onboard signer selection:
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml keys-onboard --key-id key-main-1 --state-dir ~/.greenfloor/state`
5. Run readiness checks:
   - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml doctor`
6. Run first daemon cycle:
   - `greenfloord --program-config ~/.greenfloor/config/program.yaml --markets-config ~/.greenfloor/config/markets.yaml --state-dir ~/.greenfloor/state --once`

## 2) Steady-State Operations

- Post a real offer file directly (fast path to running state):
  - Mainnet (default, pair-based): `greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1`
  - Testnet: `greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1 --network testnet11`
  - On `testnet11`, native XCH is commonly referred to as `TXCH`; CLI pair syntax remains `...:xch`.
  - Safe preflight (build only, no publish): `greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1 --dry-run`
  - If multiple markets share the same pair, rerun with explicit `--market-id`.
  - Use `--markets-config` only when overriding the default config path.
  - Publish venue is selected by `venues.offer_publish.provider` in `~/.greenfloor/config/program.yaml` (`dexie` or `splash`).
    - Optional one-off override: `--venue dexie` or `--venue splash`
    - Optional URL overrides: `--dexie-base-url ...` and `--splash-base-url ...`
- Trigger daemon reload after config edits:
  - `greenfloor-manager reload-config --state-dir ~/.greenfloor/state`
- Tune ladder/inventory safely with manager commands:
  - `set-ladder-entry`, `set-bucket-count`, `set-price-policy`
- List default supported-asset examples:
  - `greenfloor-manager --markets-config ~/.greenfloor/config/markets.yaml list-supported-assets`
- Reconcile posted offers and flag orphan/unknown entries:
  - `greenfloor-manager offers-reconcile --limit 200`
  - Optional scope: `--market-id <id>`
- View compact offer execution/reconciliation state:
  - `greenfloor-manager offers-status --limit 50 --events-limit 30`
- Export compact runtime metrics (daemon latency + offer/cancel/error rates):
  - `greenfloor-manager metrics-export --limit 500`
- Review fee budget usage:
  - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml coin-op-budget-report`

## 3) Recovery and Rollback

- List config history snapshots:
  - `greenfloor-manager config-history-list --config-path ~/.greenfloor/config/markets.yaml`
- Revert to latest snapshot:
  - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml config-history-revert --config-path ~/.greenfloor/config/markets.yaml --latest --reload --state-dir ~/.greenfloor/state --yes`
- Revert to explicit snapshot:
  - `greenfloor-manager --program-config ~/.greenfloor/config/program.yaml config-history-revert --config-path ~/.greenfloor/config/markets.yaml --backup-path ~/.greenfloor/config/.history/markets.yaml.<timestamp>.bak.yaml --reload --state-dir ~/.greenfloor/state --yes`

## 4) Expected Audit Signals

Monitor `audit_event` records in `~/.greenfloor/db/greenfloor.sqlite`:

- `xch_price_snapshot`: current price captured for strategy/cancel policy.
- `strategy_actions_planned`: deterministic action plan from strategy core.
- `strategy_offer_execution`: offer build/post execution results.
- `offer_cancel_policy`: cancel eligibility and triggered/non-triggered reasons.
- `offer_lifecycle_transition`: offer state transitions from Dexie status.
- `coin_ops_plan` and `coin_op_*`: split/combine planning and execution outcomes.

## 5) Incident Triage

- **Price unavailable:** look for `xch_price_error`; XCH planning is price-gated and may produce no actions.
- **Offer builder failures:** check `strategy_offer_execution.items[].reason` for `offer_builder_*`.
- **Dexie post/cancel issues:** look for `dexie_offers_error`, `strategy_offer_execution` skip reasons, and `offer_cancel_policy` skip reasons.
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
5. `greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1 --dry-run`
6. `greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1`
7. `greenfloor-manager offers-status --limit 50 --events-limit 30`
8. `greenfloor-manager offers-reconcile --limit 200`
9. `greenfloor-manager metrics-export --limit 500`

## 8) Testnet11 Asset Bring-Up (On-Chain Testing)

Use this checklist to stage on-chain testing while keeping GreenFloor on `chia-wallet-sdk` paths.

Optional CI workflow secret contract (`.github/workflows/live-testnet-e2e.yml`):

- Configure `TESTNET_WALLET_MNEMONIC` as an importable mnemonic phrase.
- Expected format is plain whitespace-delimited `12` or `24` words.
- Current testnet receive address: `txch1t37dk4kxmptw9eceyjvxn55cfrh827yf5f0nnnm2t6r882nkl66qknnt9k`.

1. Start with known testnet11 CAT assets that already trade on Dexie testnet.
2. Fund a testnet11 `TXCH` account (faucet) for fees and initial taker actions.
3. Acquire small test inventory in the target CAT by taking existing testnet offers.
4. Add those asset IDs to `~/.greenfloor/config/markets.yaml` as enabled test markets.
5. Run manager preflight and dry-run offer builds:
   - `greenfloor-manager config-validate`
   - `greenfloor-manager build-and-post-offer --pair <TESTCAT>:xch --size-base-units 1 --network testnet11 --dry-run`
6. Publish small-size offers and reconcile:
   - `greenfloor-manager build-and-post-offer --pair <TESTCAT>:xch --size-base-units 1 --network testnet11`
   - `greenfloor-manager offers-status --limit 50 --events-limit 30`
   - `greenfloor-manager offers-reconcile --limit 200`

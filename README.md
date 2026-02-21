# GreenFloor

GreenFloor is a long-running Python application for Chia CAT market making.

## Components

- `greenfloor-manager`: manager CLI for config validation, key checks, and reload control.
- `greenfloord`: daemon process that evaluates configured markets and emits low-inventory alerts.

## V1 Plan

- The current implementation plan is tracked in `docs/plan.md`.
- Operator deployment/recovery runbook is in `docs/runbook.md`.
- Syncing, signing, and offer-generation baseline: GreenFloor uses `chia-wallet-sdk` (included as a repo submodule) for default blockchain sync/sign and offer-file execution paths.

## Offer Files

- Offer files are plaintext Bech32m payloads with prefix `offer1...`.
- In `chia-wallet-sdk`, offer text is an encoded/compressed `SpendBundle` (`encode_offer` / `decode_offer`).
- Venue submission (`DexieAdapter.post_offer`) sends that exact offer text string as the `offer` field to `POST /v1/offers`.

## Offer Management Policy

- Every posted offer should have an expiry.
- Stable-vs-unstable pairs should use shorter expiries than other pair types.
- Explicit cancel operations are rare and policy-gated.
- Cancel is only for stable-vs-unstable pairs and only when unstable-leg price movement is strong enough to justify early withdrawal.
- Default behavior remains expiry-driven rotation instead of frequent cancel/repost churn.

## Quickstart

Bootstrap home directory first (required for real deployment):

```bash
greenfloor-manager bootstrap-home
```

Then run setup/validation and daemon commands:

```bash
python -m pip install -e ".[dev]"
greenfloor-manager --program-config config/program.yaml --markets-config config/markets.yaml config-validate
greenfloor-manager --program-config config/program.yaml --markets-config config/markets.yaml set-price-policy --market-id carbon_2022_xch_sell --set slippage_bps=90 --set min_price_quote_per_base=0.0030
greenfloor-manager --markets-config config/markets.yaml set-ladder-entry --market-id carbon_2022_xch_sell --side sell --size-base-units 10 --target-count 6 --split-buffer-count 2 --combine-when-excess-factor 2.2 --reload
greenfloor-manager --markets-config config/markets.yaml set-bucket-count --market-id carbon_2022_xch_sell --size-base-units 10 --count 4 --reload
greenfloor-manager set-low-watermark --markets-config config/markets.yaml --market-id carbon_2022_xch_sell --value 750
greenfloor-manager consolidate --markets-config config/markets.yaml --asset CARBON22 --output-count 2 --dry-run --yes
greenfloor-manager --program-config config/program.yaml coin-op-budget-report
greenfloor-manager config-history-list --config-path config/markets.yaml
# Use a backup path returned in history list:
greenfloor-manager --program-config config/program.yaml config-history-revert --config-path config/markets.yaml --backup-path config/.history/markets.yaml.<timestamp>.bak.yaml --reload --yes
# Or revert latest snapshot directly:
greenfloor-manager --program-config config/program.yaml config-history-revert --config-path config/markets.yaml --latest --reload --yes
greenfloord --program-config config/program.yaml --markets-config config/markets.yaml --once
```

Primary operator posting flow:

```bash
# Dry-run preflight
greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1 --dry-run
# Publish (mainnet default; use --network testnet11 for testnet)
greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1
```

For signer-routed coin-op execution, GreenFloor uses:
- `GREENFLOOR_WALLET_EXECUTOR_CMD` (global override executor command)
- `GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD` (source-specific override for `chia_keys`)
- `GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD` (signer backend command; defaults to `greenfloor-chia-keys-signer-backend`)
- `GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD` (signing command used by signer backend; defaults to `greenfloor-chia-keys-raw-engine-sign-impl-sdk-submit`)
- `GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD` (sdk submit implementation override command used by signer backend sign step)
- `GREENFLOOR_KEY_ID_FINGERPRINT_MAP_JSON` (optional override JSON map for key ID -> fingerprint; normally injected from `program.yaml` signer key registry by daemon path)
- `GREENFLOOR_CHIA_KEYS_DERIVATION_SCAN_LIMIT` (optional integer derivation depth scan limit used to match selected coin puzzle hashes; default `200`)

Signer key resolution contract is repo-managed through `program.yaml`:
- `keys.registry[].key_id` must match market `signer_key_id`
- `keys.registry[].fingerprint` is required for deterministic signer mapping
- optional `keys.registry[].network` is validated against `app.network`

## V1 Notifications

V1 intentionally sends only one push notification type:

- low-inventory alerts for sell-side CAT/XCH assets.

Alert content includes ticker, remaining amount, and receive address.

# GreenFloor

GreenFloor is a long-running Python application for Chia CAT market making.

## Components

- `greenfloor-manager`: manager CLI for config validation, key onboarding, cloud-wallet coin inventory/reshaping, offer building/posting, and operational checks.
- `greenfloord`: daemon process that evaluates configured markets, executes offers, and emits low-inventory alerts.

## V1 Plan

- The current implementation plan is tracked in `docs/plan.md`.
- Operator deployment/recovery runbook is in `docs/runbook.md`.
- Syncing, signing, and offer-generation baseline: GreenFloor uses `chia-wallet-sdk` (included as a repo submodule) for default blockchain sync/sign and offer-file execution paths.

## Offer Files

- Offer files are plaintext Bech32m payloads with prefix `offer1...`.
- In `chia-wallet-sdk`, offer text is an encoded/compressed `SpendBundle` (`encode_offer` / `decode_offer`).
- Manager offer publishing validates offer text with `chia-wallet-sdk` parse semantics (`Offer::from_spend_bundle`) before Dexie submission.
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
python -m pip install -e ".[dev]"
greenfloor-manager bootstrap-home
```

Validate config and check readiness:

```bash
greenfloor-manager config-validate
greenfloor-manager doctor
# Script-friendly compact JSON (default output is pretty JSON):
greenfloor-manager --json doctor
```

Onboard signing keys:

```bash
greenfloor-manager keys-onboard --key-id <your-key-id>
```

Build and post offers:

```bash
# Dry-run preflight
greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1 --dry-run
# Publish (mainnet default)
greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1
# Publish on testnet11
greenfloor-manager build-and-post-offer --pair TDBX:txch --size-base-units 1 --network testnet11
```

Cloud Wallet vault operations:

```bash
# List vault inventory (XCH + CAT)
greenfloor-manager coins-list

# Split one coin into target denominations (waits through signature + mempool + confirmation + reorg watch)
greenfloor-manager coin-split --pair TDBX:txch --coin-id <coin-id> --amount-per-coin 1000 --number-of-coins 10

# Combine small coins into one larger coin (waits through signature + mempool + confirmation + reorg watch)
greenfloor-manager coin-combine --pair TDBX:txch --number-of-coins 10 --asset-id xch
```

Coin-op wait diagnostics include:

- `signature_wait_warning` and `signature_wait_escalation` (soft-timeout style, manager keeps waiting).
- `in_mempool` with a `coinset_url` on every mempool user event.
- `confirmed` plus read-only Coinset reconciliation metadata.
- `reorg_watch_*` events while waiting for six additional blocks after first confirmation.

On `testnet11`, use `txch` as the quote symbol in pair arguments (for example `TDBX:txch`).

Check offer status and reconcile:

```bash
greenfloor-manager offers-status
greenfloor-manager offers-reconcile
```

`offers-reconcile` now emits canonical taker-detection signals from offer-state transitions (`taker_signal`) and keeps mempool/chain status pattern checks as advisory diagnostics (`taker_diagnostic`).

Run the daemon:

```bash
greenfloord --program-config config/program.yaml --markets-config config/markets.yaml --once
```

## Developer Checks

Install dev dependencies (includes `pre-commit`), then run local checks:

```bash
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -e ".[dev]"
pre-commit run --all-files
```

## Environment Variables

Operator overrides (all optional):

- `GREENFLOOR_WALLET_EXECUTOR_CMD` — override the default in-process signing path with an external executor subprocess for daemon coin-op execution.
- `GREENFLOOR_OFFER_BUILDER_CMD` — override the default in-process offer builder with an external subprocess for manager offer construction.
- `GREENFLOOR_KEY_ID_FINGERPRINT_MAP_JSON` — JSON map for key ID -> fingerprint; normally injected from `program.yaml` signer key registry by daemon path.
- `GREENFLOOR_CHIA_KEYS_DERIVATION_SCAN_LIMIT` — integer derivation depth scan limit for matching selected coin puzzle hashes (default `200`).
- `GREENFLOOR_COINSET_BASE_URL` — custom Coinset API base URL for coin queries and `push_tx`; when unset, `CoinsetAdapter` defaults to mainnet and can be forced to testnet11 by network selection.
- `coin_ops.minimum_fee_mojos` (in program config) — fallback minimum fee for coin operations when Coinset advice is unavailable (default `10000000` mojos / `0.00001 XCH`; can be set to `0`).

Cloud Wallet program config contract (`program.yaml`):

- `cloud_wallet.base_url` — GraphQL API root URL.
- `cloud_wallet.user_key_id` — user auth key id used in `chia-user-key-id`.
- `cloud_wallet.private_key_pem_path` — PEM private key path for RSA-SHA256 header signatures.
- `cloud_wallet.vault_id` — target wallet/vault ID for coin and offer operations.

CI secret for optional live testnet workflow:

- `TESTNET_WALLET_MNEMONIC` — importable wallet mnemonic used by `.github/workflows/live-testnet-e2e.yml` for `keys-onboard` mnemonic import.
- Format: whitespace-delimited `12` or `24` words (plain text mnemonic).
- Testnet receive-address example for this wallet: `txch1t37dk4kxmptw9eceyjvxn55cfrh827yf5f0nnnm2t6r882nkl66qknnt9k`.

Live `testnet11` proof workflow (CI-only mnemonic path):

- Dispatch `.github/workflows/live-testnet-e2e.yml` from GitHub Actions.
- Inputs:
  - `network_profile` (default: `testnet11`)
  - `pair` (default: `TDBX:txch`)
  - `size_base_units` (default: `1`)
  - `dry_run` (`false` to run live post/status/reconcile evidence path)
- The workflow always runs `doctor` + dry-run `build-and-post-offer`; when `dry_run=false` it additionally runs live `build-and-post-offer`, `offers-status`, and `offers-reconcile`.
- Logs are uploaded as artifact `live-testnet-e2e-artifacts`.

Testnet11 asset bootstrap helper workflow (G2):

- Dispatch `.github/workflows/testnet11-asset-bootstrap-helper.yml`.
- It discovers Dexie testnet CAT candidates and uploads `g2-testnet-asset-bootstrap-artifacts`.
- Use `markets-snippet.yaml` from the artifact as a starter config snippet for `supported_assets_example` and `markets` stanzas.

Signer key resolution contract is repo-managed through `program.yaml`:

- `keys.registry[].key_id` must match market `signer_key_id`
- `keys.registry[].fingerprint` is required for deterministic signer mapping
- optional `keys.registry[].network` is validated against `app.network`

## V1 Notifications

V1 intentionally sends only one push notification type:

- low-inventory alerts for sell-side CAT/XCH assets.

Alert content includes ticker, remaining amount, and receive address.

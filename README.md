# GreenFloor

GreenFloor is a long-running Python application for Chia CAT market making.

## Components

- `greenfloor-manager`: manager CLI for config validation, key onboarding, offer building/posting, and operational checks.
- `greenfloord`: daemon process that evaluates configured markets, executes offers, and emits low-inventory alerts.

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
python -m pip install -e ".[dev]"
greenfloor-manager bootstrap-home
```

Validate config and check readiness:

```bash
greenfloor-manager config-validate
greenfloor-manager doctor
```

Onboard signing keys:

```bash
greenfloor-manager keys-onboard --key-id <your-key-id>
```

Build and post offers:

```bash
# Dry-run preflight
greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1 --dry-run
# Publish (mainnet default; use --network testnet11 for testnet)
greenfloor-manager build-and-post-offer --pair CARBON22:xch --size-base-units 1
```

Check offer status and reconcile:

```bash
greenfloor-manager offers-status
greenfloor-manager offers-reconcile
```

Run the daemon:

```bash
greenfloord --program-config config/program.yaml --markets-config config/markets.yaml --once
```

## Environment Variables

Operator overrides (all optional):

- `GREENFLOOR_WALLET_EXECUTOR_CMD` — override the default in-process signing path with an external executor subprocess for daemon coin-op execution.
- `GREENFLOOR_OFFER_BUILDER_CMD` — override the default in-process offer builder with an external subprocess for manager offer construction.
- `GREENFLOOR_KEY_ID_FINGERPRINT_MAP_JSON` — JSON map for key ID -> fingerprint; normally injected from `program.yaml` signer key registry by daemon path.
- `GREENFLOOR_CHIA_KEYS_DERIVATION_SCAN_LIMIT` — integer derivation depth scan limit for matching selected coin puzzle hashes (default `200`).
- `GREENFLOOR_WALLET_SDK_COINSET_URL` — custom coinset RPC URL (overrides mainnet/testnet defaults).

Signer key resolution contract is repo-managed through `program.yaml`:
- `keys.registry[].key_id` must match market `signer_key_id`
- `keys.registry[].fingerprint` is required for deterministic signer mapping
- optional `keys.registry[].network` is validated against `app.network`

## V1 Notifications

V1 intentionally sends only one push notification type:

- low-inventory alerts for sell-side CAT/XCH assets.

Alert content includes ticker, remaining amount, and receive address.

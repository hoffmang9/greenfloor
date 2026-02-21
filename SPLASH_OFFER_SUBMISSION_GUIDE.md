# Splash Offer Submission Guide

This guide explains how this repo submits Chia offers to a Splash server, including local (`localhost`) and local network (LAN) setups.

**As of:** 2026-02-19

## What This Repo Actually Does

Offer submission is centralized in `old/common.py`:

- Function: `post_offer_to_splash(offer_text: str)`
- Transport: HTTP `POST`
- URL source: `SPLASH_API_URL` (env var with fallback default)
- Body: JSON with a single `offer` field

Current default in code (when env var is not set):

- `SPLASH_API_URL = "http://john-deere.hoffmang.com:4000"`

## Endpoint and Request Format

- Method: `POST`
- URL: value of `SPLASH_API_URL` (base URL on port `4000`)
- Header: `Content-Type: application/json`
- Body:

```json
{
  "offer": "<serialized_chia_offer_string>"
}
```

## Configure Splash for Localhost or LAN

Preferred: set `SPLASH_API_URL` as an environment variable.

- Local machine:
  - `export SPLASH_API_URL="http://localhost:4000"`
- LAN host:
  - `export SPLASH_API_URL="http://192.168.1.50:4000"` (replace with your Splash host IP)

Alternative: edit `old/common.py` fallback default.

If both are present, env var wins.

Legacy/manual edit examples:

- Local machine:
  - `http://localhost:4000`
- LAN host:
  - `http://192.168.1.50:4000` (replace with your Splash host IP)

Example export:

```bash
export SPLASH_API_URL="http://localhost:4000"
python old/make_offer.py --offer-wallet <CAT_ASSET_ID> --pair xch --count 1
```

Example code fallback edit:

```python
SPLASH_API_URL = "http://localhost:4000"
```

## How Offers Are Created Then Submitted

The scripts create an offer via Chia wallet RPC, then submit it to Splash:

1. Build/create offer using:
   - `chia rpc wallet create_offer_for_ids`
2. Extract:
   - `offer` (serialized offer string)
   - `trade_id`
3. Submit `offer` to Splash with `post_offer_to_splash(...)`

Primary scripts that do this:

- `old/make_offer.py` (recommended unified script)
- `old/make_offer_for_xch.py` (legacy)
- `old/make_offer_for_b_usdc.py` (legacy)

## Quick Usage (From This Repo)

From the `old/` directory:

```bash
python make_offer.py --offer-wallet <CAT_ASSET_ID> --pair xch --count 1
```

or:

```bash
python make_offer.py --offer-wallet <CAT_ASSET_ID> --pair usdc --count 1
```

If the command succeeds, output includes:

- created offer string
- trade ID
- Splash API response

## Minimal Manual POST Test

Use this when you already have an offer string:

```bash
curl -X POST "http://localhost:4000" \
  -H "Content-Type: application/json" \
  -d '{"offer":"offer1..."}'
```

## Troubleshooting

- `Failed to post offer to Splash`:
  - check host/IP and port (`4000`)
  - verify Splash server is running and reachable
  - check local firewall / router rules
- Connection works locally but not from another machine:
  - bind Splash to a LAN-reachable interface, not loopback-only
  - use LAN IP, not `localhost`, from remote clients
- Offer creation fails before posting:
  - ensure Chia wallet RPC is running
  - verify `CHIA_BIN_PATH` in `old/common.py`
- Slow response/timeouts:
  - repo timeout is `REQUEST_TIMEOUT = 30` seconds in `old/common.py`

## Notes and Caveats

- `SPLASH_API_URL` now supports env override; if unset, code falls back to the default URL in `old/common.py`.
- No auth headers/tokens are used in the existing submission implementation.
- `make_offer.py` can use `--cancel-after-create`; cancellation happens after creation and around the same flow as posting, so behavior depends on timing and server-side handling.

## Source References in This Repo

- `old/common.py`
- `old/make_offer.py`
- `old/make_offer_for_xch.py`
- `old/make_offer_for_b_usdc.py`
- `old/README.md`

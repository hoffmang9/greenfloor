# Dexie.space Docs & API Reference

This file summarizes the public API docs at `https://dexie.space/api` and related Dexie API pages.

**As of:** 2026-02-19

## Overview

- Dexie provides public APIs for Chia offers, swaps, and CAT price feeds.
- Auth is not required for the public APIs, but abusive IPs may be rate limited.
- Main API base URL (mainnet): `https://api.dexie.space`
- Testnet base URL (offers docs): `https://api-testnet.dexie.space`
- WebSocket stream endpoint (offers streaming): `wss://api.dexie.space/v1/stream`

## Docs Structure (Observed)

- Offers API docs: `https://dexie.space/api`
- Swap API docs: `https://dexie.space/api/swap`
- Prices API docs: `https://dexie.space/api/prices`
- Status page: `https://status.dexie.space/`

## API Families

- `v1` - offers, rewards, swap, stream
- `v3` - prices (pairs, tickers, orderbook, historical trades)

## Offer Status Codes

- `0` Open
- `1` Pending
- `2` Cancelling
- `3` Cancelled
- `4` Completed
- `5` Unknown
- `6` Expired

## Endpoint Catalog (Verified)

### Offers (`/v1`)

- `POST /v1/offers` - post an offer
- `GET /v1/offers` - search offers
- `GET /v1/offers/:id` - inspect a specific offer
- `POST /v1/rewards/check` - query liquidity rewards
- `POST /v1/rewards/claim` - claim liquidity rewards
- `wss://api.dexie.space/v1/stream` - websocket streaming API (pro)

### Swap (`/v1`)

- `GET /v1/swap/quote` - get a swap quote
- `POST /v1/swap` - execute a swap with an offer
- `GET /v1/swap/tokens` - list supported swap tokens

### Prices (`/v3/prices`)

- `GET /v3/prices/pairs` - list traded CAT/XCH pairs
- `GET /v3/prices/tickers` - market and ticker data
- `GET /v3/prices/orderbook` - order book depth
- `GET /v3/prices/historical_trades` - historical trades

## Integration Cheat Sheet (Required Inputs)

### Offers

| Endpoint                          | Required input                                               |
| --------------------------------- | ------------------------------------------------------------ |
| `POST /v1/offers`                 | `offer`                                                      |
| `GET /v1/offers`                  | none                                                         |
| `GET /v1/offers/:id`              | `id` path param                                              |
| `POST /v1/rewards/check`          | see docs (body schema not fully captured in fetched content) |
| `POST /v1/rewards/claim`          | see docs (body schema not fully captured in fetched content) |
| `wss://api.dexie.space/v1/stream` | websocket connect/subscribe flow                             |

### Swap

| Endpoint              | Required input                                        |
| --------------------- | ----------------------------------------------------- |
| `GET /v1/swap/quote`  | `from`, `to`, and one of `from_amount` or `to_amount` |
| `POST /v1/swap`       | `offer`                                               |
| `GET /v1/swap/tokens` | none                                                  |

### Prices

| Endpoint                           | Required input                                                   |
| ---------------------------------- | ---------------------------------------------------------------- |
| `GET /v3/prices/pairs`             | none                                                             |
| `GET /v3/prices/tickers`           | none (`ticker_id` optional)                                      |
| `GET /v3/prices/orderbook`         | `ticker_id` (`depth` optional)                                   |
| `GET /v3/prices/historical_trades` | `ticker_id` (`type`, `limit`, `start_time`, `end_time` optional) |

## Key Request/Response Notes

- `POST /v1/offers` supports optional:
  - `drop_only` (faster response, primarily offer id)
  - `claim_rewards` (maker liquidity rewards automation)
- For GreenFloor integration, the `offer` body value is an Offer-file string (`offer1...`) produced by `chia-wallet-sdk` offer encoding.
- GreenFloor offer strategy is expiry-first: all offers expire, with shorter expiries on stable-vs-unstable pairs.
- GreenFloor cancel path is intentionally rare and policy-gated (stable-vs-unstable pairs only; triggered by strong unstable-leg price movement).
- `GET /v1/offers` returns paginated payload including:
  - `success`, `count`, `page`, `page_size`, `offers[]`
- `GET /v1/swap/quote` response includes:
  - `quote.from`, `quote.from_amount`, `quote.to`, `quote.to_amount`
  - `quote.combination_fee`, `quote.suggested_tx_fee`
- `GET /v3/prices/orderbook` response includes:
  - `orderbook.ticker_id`, `pool_id`, `timestamp`, `bids`, `asks`
- `GET /v3/prices/historical_trades` response includes:
  - `success`, `ticker_id`, `pool_id`, `timestamp`, `trades[]`

## Example Calls

```bash
curl "https://api.dexie.space/v1/offers"
```

```bash
curl "https://api.dexie.space/v1/swap/quote?from=xch&to=<asset_id>&to_amount=1000"
```

```bash
curl "https://api.dexie.space/v3/prices/tickers"
```

```bash
wscat -c wss://api.dexie.space/v1/stream
```

## TypeScript Quickstart

```ts
const DEXIE_API = "https://api.dexie.space";

async function getJson<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status} for ${url}`);
  return (await res.json()) as T;
}
```

### Get Offers

```ts
type OffersResponse = {
  success: boolean;
  count: number;
  page: number;
  page_size: number;
  offers: Array<{ id: string; status: number }>;
};

const offers = await getJson<OffersResponse>(
  "https://api.dexie.space/v1/offers",
);
console.log("offers:", offers.offers.length);
```

### Get Swap Quote

```ts
type SwapQuoteResponse = {
  success: boolean;
  quote: {
    from: string;
    from_amount: number;
    to: string;
    to_amount: number;
    combination_fee: number;
    suggested_tx_fee: number;
  };
};

const toAssetId =
  "db1a9020d48d9d4ad22631b66ab4b9ebd3637ef7758ad38881348c5d24c38f20";
const quoteUrl = `${DEXIE_API}/v1/swap/quote?from=xch&to=${toAssetId}&to_amount=1000`;

const quote = await getJson<SwapQuoteResponse>(quoteUrl);
console.log("required from_amount:", quote.quote.from_amount);
```

### Get Tickers

```ts
type TickersResponse = {
  success: boolean;
  tickers: Array<{
    ticker_id: string;
    base_code: string;
    target_code: string;
    last_price: string;
    bid: string | null;
    ask: string | null;
  }>;
};

const tickers = await getJson<TickersResponse>(
  "https://api.dexie.space/v3/prices/tickers",
);
console.log("tickers:", tickers.tickers.length);
```

### Post Offer

```ts
type PostOfferRequest = {
  offer: string;
  drop_only?: boolean;
  claim_rewards?: boolean;
};

type PostOfferResponse = {
  success: boolean;
  id?: string;
  error?: string;
};

async function postOffer(
  payload: PostOfferRequest,
): Promise<PostOfferResponse> {
  const res = await fetch(`${DEXIE_API}/v1/offers`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status} while posting offer`);
  return (await res.json()) as PostOfferResponse;
}

const posted = await postOffer({
  offer: "offer1...", // serialized Chia offer string
  drop_only: true,
});
console.log("posted offer id:", posted.id);
```

### Execute Swap

```ts
type ExecuteSwapRequest = {
  offer: string;
  fee_destination?: string; // xch1...
};

type ExecuteSwapResponse = {
  success: boolean;
  offer_id?: string;
  error?: string;
};

async function executeSwap(
  payload: ExecuteSwapRequest,
): Promise<ExecuteSwapResponse> {
  const res = await fetch(`${DEXIE_API}/v1/swap`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status} while executing swap`);
  return (await res.json()) as ExecuteSwapResponse;
}

const swapResult = await executeSwap({
  offer: "offer1...", // offer created from quote flow
});
console.log("swap success:", swapResult.success);
```

## Common Pitfalls

- Amounts are generally in mojos (smallest units), not display units.
- For swaps, pass exactly one target amount direction (`from_amount` or `to_amount`) to avoid ambiguous quoting.
- `v1` and `v3` APIs are separate families; do not mix endpoint paths.
- High-volume consumers should plan for rate limits and retry/backoff.
- Large responses (for example all offers or all tickers) can be heavy; use query filters where supported.

## Source Pages

- https://dexie.space/api
- https://dexie.space/api/swap
- https://dexie.space/api/prices
- https://status.dexie.space/
- https://api.dexie.space/v1/offers
- https://api.dexie.space/v1/swap/tokens
- https://api.dexie.space/v1/swap/quote?from=xch&to=db1a9020d48d9d4ad22631b66ab4b9ebd3637ef7758ad38881348c5d24c38f20&to_amount=1000
- https://api.dexie.space/v3/prices/pairs
- https://api.dexie.space/v3/prices/tickers
- https://api.dexie.space/v3/prices/orderbook?ticker_id=db1a9020d48d9d4ad22631b66ab4b9ebd3637ef7758ad38881348c5d24c38f20_xch&depth=10
- https://api.dexie.space/v3/prices/historical_trades?ticker_id=db1a9020d48d9d4ad22631b66ab4b9ebd3637ef7758ad38881348c5d24c38f20_xch&limit=5

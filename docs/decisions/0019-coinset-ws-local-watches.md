# ADR 0019: Coinset WebSocket local watches (no webhooks)

## Status

Accepted (2026-07-08)

## Context

GreenFloor previously mixed Dexie as the default offer publish venue with Coinset
HTTP/WebSocket signals that were mostly audit-oriented. Dead YAML fields
(`webhook_*`, unwired `mempool_monitor`) suggested an HTTP webhook path that was
never the operator transport.

## Decision

1. **Publish venue:** optional `coinset | dexie | splash`; **default `coinset`** via
   `POST /push_offer`. Mark offers `open` as soon as Coinset accepts. Canonical
   offer id is the 64-hex spend-bundle hash (Coinset `offer_id` / Dexie `trade_id`).
2. **Inbound signals:** Coinset WebSocket only (`events=transaction,offer` +
   `tx_status=pending,confirmed` + stable market `p2` filters). No HTTP webhooks /
   API keys.
3. **Watches:** durable SQLite `offer_coin_watches` registered at post (maker coins +
   known maker p2s). WS offer events and watch hits drive lifecycle transitions
   directly; Dexie reconcile remains backfill.
4. **Cancel:** local cancel + `POST /push_tx`; watch cancel on WS. Do not submit
   spends over WebSocket.
5. **Inventory:** WS p2/coin hits mark inventory stale; skip blind HTTP polls within
   90s max-staleness and reuse last bucket counts when fresh.

## Consequences

- Operators configure `venues.offer_publish.provider` and websocket URL only.
- Webhook listen addresses and mempool_monitor YAML are removed.
- Mainnet-first; testnet11 WS hardening deferred.
- Daemon loop and CLI `--once` both build `InventoryP2Index` from markets before
  WS capture/subscribe. Config reload rebuilds the index and reconnects WS so
  p2 filters stay current without a process restart.

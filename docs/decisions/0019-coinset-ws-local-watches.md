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
3. **Watches:** durable SQLite `offer_coin_watches` registered atomically at post
   (maker coins + known maker p2s such as fixed delegated puzzle hash). Schema
   migration backfills missing watches from cancel/presplit metadata for pre-upgrade
   rows. Shared market inventory receive/CAT outer p2s are **not** stored on
   per-offer watches; `InventoryP2Index` still drives WS filters and inventory
   freshness. Optional coin-id fields on transaction frames are matched when present.
   WS offer events and watch hits drive lifecycle transitions directly.
4. **Dexie reconcile:** only for Dexie-authoritative watched offers (explicit
   `publish_venue=dexie`). Schema migration backfills legacy `NULL` venues from
   offer-id shape once; authority checks no longer infer from id shape at runtime.
   Coinset/splash offers skip Dexie HTTP entirely. Dexie list matching uses
   `trade_id` ∪ bech32 `id`.
5. **Cancel:** local cancel + `POST /push_tx`; watch cancel on WS. Cancel targets
   come from local cancel-eligible offer state (Coinset/splash included). Dexie
   venue offers additionally require Dexie list status open (status index built
   once in reconcile from `trade_id` ∪ bech32 `id`). Cancel spend construction
   prefers local offer file or Coinset + stored cancel metadata; Dexie offer-file
   fetch is optional fallback only. Do not submit spends over WebSocket.
6. **Inventory:** WS p2/coin hits mark inventory stale; skip blind HTTP polls within
   90s max-staleness and reuse last bucket counts when fresh. Durable watches are
   registered atomically at post and backfilled/healed once on schema open
   (`INSERT OR IGNORE` for missing coin and p2 rows); coin-ops only reads the
   watch table for do-not-touch.

## Consequences

- Operators configure `venues.offer_publish.provider` and websocket URL only.
- Webhook listen addresses and mempool_monitor YAML are removed.
- Mainnet-first; testnet11 WS hardening deferred.
- Daemon loop and CLI `--once` both build `InventoryP2Index` from markets before
  WS capture/subscribe. Config reload rebuilds the index and reconnects WS so
  p2 filters stay current without a process restart. The `config_reloaded` audit
  payload includes `inventory_p2_rebuild: ok|failed`; a failed rebuild keeps prior
  filters.

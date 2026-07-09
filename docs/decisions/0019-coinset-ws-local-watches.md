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
   (maker coins + on-chain maker puzzle hashes — CAT outer or XCH p2 — not the
   fixed delegated CONDITIONS hash). Schema migration backfills missing watches
   from cancel/presplit metadata for pre-upgrade rows. Shared market inventory
   receive/CAT outer p2s are **not** stored on per-offer watches; `InventoryP2Index`
   still drives WS filters and inventory freshness. Optional coin-id fields on
   transaction frames are matched when present. WS offer events and watch hits
   drive lifecycle transitions directly. Cancel submit prepares `cancel_submitted`
   before broadcast (watches kept), then observes the cancel tx after successful
   `push_tx` (watches kept until terminal persist). Pure watch hits while
   `cancel_submitted` are ignored by cancel policy, so cancel-spend reuse of maker
   keys cannot look like taker activity. Watch backfill skips `cancel_submitted`
   rows. Venue backfill never labels 64-hex ids as `coinset` (Dexie `trade_id`
   shares that shape) and never mass-clears explicit `publish_venue=coinset`; it
   only sets `dexie` for unambiguous non-64-hex legacy NULL ids (via `schema_meta`
   `watch_venue_backfill_v2`). Missing watches are healed each reconcile via a
   single `MarketWatchPlan` scan: cancel metadata first (all venues), then Dexie
   payloads for heal-only NULL/`dexie` gaps (`fetch_and_ensure_watches` seeds both
   maker coin ids and on-chain maker p2s from cancellable offer inputs; when a
   list row lacks a decodable `offer1…`, heal calls `get_offer` so watches are
   not stuck coin-only; no Dexie lifecycle). Dexie lifecycle remains
   `publish_venue=dexie` only.
4. **Dexie reconcile:** only for Dexie-authoritative watched offers (explicit
   `publish_venue=dexie`). Authority checks use persisted venue only (no id-shape
   heuristics at runtime). Coinset/splash / NULL venues skip Dexie lifecycle
   transitions. Dexie list matching uses `trade_id` ∪ bech32 `id`. Size/status
   indexes are built from authoritative payloads only.
5. **Cancel:** local cancel + `POST /push_tx`; watch cancel on WS. Cancel targets
   come from local cancel-eligible offer state (Coinset/splash included). Dexie
   venue offers additionally require Dexie list status open (status index built
   once in reconcile from `trade_id` ∪ bech32 `id`). Orphan `cancel_submitted`
   past grace resets to `open` (`REASON_CANCEL_SUBMIT_STALE_ORPHAN`) for
   Coinset/splash (no Dexie status) and Dexie-open alike. Cancel spend
   construction prefers local offer file or Coinset + stored cancel metadata;
   Dexie offer-file fetch is optional fallback only. Do not submit spends over
   WebSocket.
6. **Inventory:** WS p2/coin hits mark inventory stale; skip blind HTTP polls within
   90s max-staleness and reuse last bucket counts when fresh. Durable watches are
   registered atomically at post, backfilled once on schema open via
   `schema_meta` (`watch_venue_backfill_v2`), and healed each reconcile via
   `classify_and_heal_local` + heal-only Dexie fetch. Coin-ops excludes durable
   `kind='coin'` watch ids and durable `kind='p2'` maker puzzle hashes inside
   `list_spendable_coins` (local on-chain `puzzle_hash`; no network expand).
   Explicit CLI coin ids are refused when they match durable maker coin watches.

## Consequences

- Operators configure `venues.offer_publish.provider` and websocket URL only.
- Webhook listen addresses and mempool_monitor YAML are removed.
- Mainnet-first; testnet11 WS hardening deferred.
- Daemon loop and CLI `--once` both build shared WS state via
  `CoinsetWsShared::from_markets_or_empty` before WS capture/subscribe. Bad
  markets are skipped with a warning so a single bad receive/base_asset does not
  abort the process; a total index build failure starts with empty filters.
  Config reload rebuilds the index and reconnects WS so p2 filters stay current
  without a process restart. The `config_reloaded` audit payload includes
  `inventory_p2_rebuild: ok|failed`; a failed rebuild keeps prior filters.

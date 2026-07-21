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
   `POST /push_offer`. Persist `open` + durable watches immediately after venue
   accept through the caller's synchronous store sink (the daemon uses its
   cycle write store), before audit flush. A persist failure is recorded as a
   post failure; batch flush writes audits only and never replays offer state
   or watches. Canonical offer id is the 64-hex spend-bundle
   hash (Coinset `offer_id` / Dexie `trade_id`).
2. **Inbound signals:** Coinset WebSocket (`events=transaction,offer` +
   `tx_status=pending,confirmed`) with `p2` filters equal to the union of stable
   market inventory p2s and durable maker p2 watches. No HTTP webhooks / API keys.
   HTTP `get_transaction` polling supplements WS by confirming prepared
   `cancel_submitted` transaction ids during recovery and every cycle preamble.
3. **Watches:** durable SQLite `offer_coin_watches` registered atomically at post
   (maker coins + on-chain maker puzzle hashes — CAT outer or XCH p2 — not the
   fixed delegated CONDITIONS hash), sourced from required `OfferCancelFields` on
   every successful create (Direct: single exact-size maker coin; presplit: split
   coin + fixed CONDITIONS hash). Schema migration backfills missing watches
   from cancel metadata for pre-upgrade rows. Shared market inventory
   receive/CAT outer p2s are **not** stored on per-offer watches; `InventoryP2Index`
   still drives WS filters and inventory freshness. Optional coin-id fields on
   transaction frames are matched when present. WS offer events and watch hits
   drive lifecycle transitions directly. Cancel submit prepares `cancel_submitted`
   before broadcast (watches kept), then observes the cancel tx after successful
   `push_tx` (watches kept until terminal persist). Offer-frame `pending` (with or
   without `tx_id`) seeds `tx_signal_state` only and does **not** advance lifecycle
   to `mempool_observed` — that state ages out of active-slot counts after three
   minutes while a Coinset listing can still be live, which would allow duplicate
   ladder posts. Take detection stays on durable maker **coin** watch hits
   (`MakerHit::{Mempool,Confirmed}`: pending → `mempool_observed`; confirmed →
   `tx_block_confirmed`, including when Coinset omits spend-bundle `ids` but
   still lists maker coin removals). P2-only hits mark inventory stale only and do
   **not** advance lifecycle — shared maker puzzle hashes can match every open
   offer on a market when Coinset omits coin ids. While `cancel_submitted`,
   unattributed confirmed maker hits preserve
   (`REASON_CANCEL_SUBMIT_CONFIRMED_MAKER_HIT_IGNORED`, await HTTP cancel confirm)
   and never orphan-unwedge to `open`; pure mempool maker hits / cancel-tx-only
   mempool are ignored within grace. Offer-frame `confirmed` / terminal statuses
   still drive lifecycle. Watch backfill skips `cancel_submitted` rows. Venue
   backfill never labels 64-hex ids as `coinset` (Dexie `trade_id` shares that
   shape) and never mass-clears explicit `publish_venue=coinset`; it only sets
   `dexie` for unambiguous non-64-hex legacy NULL ids (via `schema_meta`
   `watch_venue_backfill_v2`). Missing watches are healed each reconcile via a
   single `prepare_market_reconcile_local` scan: cancel-submitted collection,
   cancel-metadata heal, and Dexie role classify (`DexieWatchRoles`), then Dexie
   payloads for heal-only NULL-venue gaps (`fetch_and_ensure_watches` seeds both
   maker coin ids and on-chain maker p2s from cancellable offer inputs; when a
   list row lacks a decodable `offer1…`, heal calls `get_offer` so watches are
   not stuck coin-only; no Dexie lifecycle). Dexie lifecycle remains
   `publish_venue=dexie` only and applies through the same
   `apply_watched_offer_signals` spine as Coinset WS (CLI Dexie reconcile is
   single-pass fetch → signals → apply). Dexie-authoritative rows heal missing
   watches from the same payload used for lifecycle augment. Schema migration
   drops legacy `presplit_input_coin_id` after copying into `cancel_input_coin_id`.
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
   Coinset/splash (no Dexie status) and Dexie-open alike. Daemon reconcile collects
   those rows in `prepare_market_reconcile_local` and applies empty-signal
   cancel-submitted policy so all venues unwedge without Dexie HTTP lifecycle or a
   WS confirm frame. Each cycle also polls Coinset HTTP `get_transaction` for
   prepared cancel ids, ingests confirmed signals, and promotes matching rows.
   Cancel spend
   construction prefers local offer file or Coinset + stored cancel metadata;
   Dexie offer-file fetch is optional fallback only. Do not submit spends over
   WebSocket.
6. **Inventory:** WS inventory-index `p2` hits, durable maker watch hits, offer-frame
   `confirmed`/`cancelled`/`expired`, and cancel-tx confirmation all mark inventory
   stale; skip blind HTTP polls within 90s max-staleness and reuse last bucket counts
   when fresh. Offer-frame `p2s` alone do not mark inventory stale. Durable watches are
   registered atomically at post, backfilled once on schema open via
   `schema_meta` (`watch_venue_backfill_v2`), and healed each reconcile via
   `prepare_market_reconcile_local` + heal-only Dexie fetch. Coin-ops excludes durable
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

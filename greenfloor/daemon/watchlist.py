"""Watchlist coin-id helpers and Dexie size maps (active-offer counting is Rust-only)."""

from __future__ import annotations

from greenfloor.runtime import offer_watchlist as _offer_watchlist

build_dexie_size_by_offer_id = _offer_watchlist.build_dexie_size_by_offer_id
match_watched_coin_ids = _offer_watchlist.match_watched_coin_ids
set_watched_coin_ids_for_market = _offer_watchlist.set_watched_coin_ids_for_market
update_market_coin_watchlist_from_dexie = _offer_watchlist.update_market_coin_watchlist_from_dexie
watchlist_offer_ids_from_store = _offer_watchlist.watchlist_offer_ids_from_store
watched_coin_ids_for_market = _offer_watchlist.watched_coin_ids_for_market

# Legacy private aliases for remaining Python glue modules.
_match_watched_coin_ids = match_watched_coin_ids
_watched_coin_ids_for_market = watched_coin_ids_for_market

__all__ = [
    "build_dexie_size_by_offer_id",
    "match_watched_coin_ids",
    "set_watched_coin_ids_for_market",
    "update_market_coin_watchlist_from_dexie",
    "watchlist_offer_ids_from_store",
    "watched_coin_ids_for_market",
]

"""Rust-backed active-offer counting and coin watchlist patch points."""

from __future__ import annotations

from datetime import datetime
from typing import Any

from greenfloor.core.engine_bridge import db_path_from_store, import_engine, require_engine_method

_WATCHLIST_MISSING = "watchlist"


def _watchlist_method(name: str):
    return require_engine_method(import_engine(), name, missing=_WATCHLIST_MISSING)


def new_coin_watchlist_cache() -> Any:
    cache_cls = _watchlist_method("CoinWatchlistCache")
    return cache_cls()


def build_dexie_size_by_offer_id(
    offers: list[dict[str, Any]], base_asset_id: str
) -> dict[str, int]:
    fn = _watchlist_method("build_dexie_size_by_offer_id")
    return dict(fn(offers, base_asset_id))


def match_watched_coin_ids(
    *,
    coin_watchlist: Any,
    observed_coin_ids: list[str],
) -> dict[str, list[str]]:
    fn = _watchlist_method("match_watched_coin_ids")
    payload = fn(coin_watchlist, observed_coin_ids)
    return {str(market_id): list(coin_ids) for market_id, coin_ids in dict(payload).items()}


def set_watched_coin_ids_for_market(
    *,
    coin_watchlist: Any,
    market_id: str,
    coin_ids: set[str],
) -> None:
    fn = _watchlist_method("set_watched_coin_ids_for_market")
    fn(coin_watchlist, market_id, sorted(coin_ids))


def update_market_coin_watchlist_from_dexie(
    *,
    market: Any,
    offers: list[dict[str, Any]],
    store: Any,
    coin_watchlist: Any | None = None,
) -> None:
    cache = coin_watchlist or new_coin_watchlist_cache()
    fn = _watchlist_method("update_market_coin_watchlist_from_offers")
    fn(db_path_from_store(store), cache, market.market_id, offers)


def active_offer_counts_by_size(
    *,
    store: Any,
    market_id: str,
    clock: datetime,
    dexie_size_by_offer_id: dict[str, int] | None = None,
    tracked_sizes: set[int] | None = None,
) -> tuple[dict[int, int], dict[str, int], int]:
    payload = _watchlist_method("active_offer_counts_by_size")(
        db_path_from_store(store),
        market_id,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
        tracked_sizes=sorted(tracked_sizes) if tracked_sizes is not None else None,
        clock_iso=clock.isoformat(),
    )
    counts = {int(size): int(count) for size, count in dict(payload["counts_by_size"]).items()}
    state_counts = {str(k): int(v) for k, v in dict(payload["state_counts"]).items()}
    return counts, state_counts, int(payload["unmapped"])


def active_offer_counts_by_size_and_side(
    *,
    store: Any,
    market_id: str,
    clock: datetime,
    dexie_size_by_offer_id: dict[str, int] | None = None,
    tracked_sizes: set[int] | None = None,
) -> tuple[dict[str, dict[int, int]], dict[str, int], int]:
    payload = _watchlist_method("active_offer_counts_by_size_and_side")(
        db_path_from_store(store),
        market_id,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
        tracked_sizes=sorted(tracked_sizes) if tracked_sizes is not None else None,
        clock_iso=clock.isoformat(),
    )
    counts_by_side = {
        side: {int(size): int(count) for size, count in dict(sizes).items()}
        for side, sizes in dict(payload["counts_by_side"]).items()
    }
    state_counts = {str(k): int(v) for k, v in dict(payload["state_counts"]).items()}
    return counts_by_side, state_counts, int(payload["unmapped"])


__all__ = [
    "active_offer_counts_by_size",
    "active_offer_counts_by_size_and_side",
    "build_dexie_size_by_offer_id",
    "match_watched_coin_ids",
    "new_coin_watchlist_cache",
    "set_watched_coin_ids_for_market",
    "update_market_coin_watchlist_from_dexie",
]

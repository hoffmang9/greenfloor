"""Rust-backed offer watchlist helpers (PyO3)."""

from __future__ import annotations

from typing import Any

from greenfloor.core.cycle import is_dexie_offer_missing_error_text
from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.storage.sqlite import SqliteStore

__all__ = [
    "RESEED_MEMPOOL_MAX_AGE_SECONDS",
    "build_dexie_size_by_offer_id",
    "is_dexie_offer_missing_error",
    "match_watched_coin_ids",
    "new_coin_watchlist_cache",
    "set_watched_coin_ids_for_market",
    "update_market_coin_watchlist_from_dexie",
    "watched_coin_ids_for_market",
    "watchlist_offer_ids_from_store",
]

_WATCHLIST_MISSING = "watchlist"


def _watchlist_method(name: str):
    return require_engine_method(import_engine(), name, missing=_WATCHLIST_MISSING)


def _engine_constant(name: str) -> int:
    value = getattr(import_engine(), name, None)
    if value is None:
        raise RuntimeError(f"engine missing watchlist constant: {name}")
    return int(value)


RESEED_MEMPOOL_MAX_AGE_SECONDS = _engine_constant("RESEED_MEMPOOL_MAX_AGE_SECONDS")


def new_coin_watchlist_cache() -> Any:
    cache_cls = _watchlist_method("CoinWatchlistCache")
    return cache_cls()


def is_dexie_offer_missing_error(error: Exception) -> bool:
    return is_dexie_offer_missing_error_text(str(error))


def watchlist_offer_ids_from_store(*, store: SqliteStore, market_id: str) -> set[str]:
    from greenfloor.core.engine_bridge import db_path_from_store

    fn = _watchlist_method("watchlist_offer_ids_from_store")
    return set(fn(db_path_from_store(store), market_id))


def set_watched_coin_ids_for_market(
    *,
    coin_watchlist: Any,
    market_id: str,
    coin_ids: set[str],
) -> None:
    fn = _watchlist_method("set_watched_coin_ids_for_market")
    fn(coin_watchlist, market_id, sorted(coin_ids))


def match_watched_coin_ids(
    *,
    coin_watchlist: Any,
    observed_coin_ids: list[str],
) -> dict[str, list[str]]:
    fn = _watchlist_method("match_watched_coin_ids")
    payload = fn(coin_watchlist, observed_coin_ids)
    return {str(market_id): list(coin_ids) for market_id, coin_ids in dict(payload).items()}


def watched_coin_ids_for_market(*, coin_watchlist: Any, market_id: str) -> set[str]:
    fn = _watchlist_method("watched_coin_ids_for_market")
    return set(fn(coin_watchlist, market_id))


def update_market_coin_watchlist_from_dexie(
    *,
    market: Any,
    offers: list[dict[str, Any]],
    store: SqliteStore,
    coin_watchlist: Any | None = None,
) -> None:
    from greenfloor.core.engine_bridge import db_path_from_store

    cache = coin_watchlist or new_coin_watchlist_cache()
    fn = _watchlist_method("update_market_coin_watchlist_from_offers")
    fn(db_path_from_store(store), cache, market.market_id, offers)


def build_dexie_size_by_offer_id(
    offers: list[dict[str, Any]], base_asset_id: str
) -> dict[str, int]:
    fn = _watchlist_method("build_dexie_size_by_offer_id")
    return dict(fn(offers, base_asset_id))

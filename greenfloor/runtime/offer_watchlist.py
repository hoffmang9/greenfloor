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


def _engine():
    return import_engine()


def _require(name: str, *, missing: str):
    return require_engine_method(_engine(), name, missing=missing)


def _db_path(store: SqliteStore) -> str:
    db_path = getattr(store, "db_path", None)
    if db_path is None:
        raise TypeError("watchlist helpers require SqliteStore with db_path")
    return str(db_path)


def new_coin_watchlist_cache() -> Any:
    cache_cls = _require("CoinWatchlistCache", missing="coin watchlist cache")
    return cache_cls()


def reseed_mempool_max_age_seconds() -> int:
    fn = _require("reseed_mempool_max_age_seconds", missing="watchlist")
    return int(fn())


RESEED_MEMPOOL_MAX_AGE_SECONDS = 3 * 60


def is_dexie_offer_missing_error(error: Exception) -> bool:
    return is_dexie_offer_missing_error_text(str(error))


def watchlist_offer_ids_from_store(*, store: SqliteStore, market_id: str) -> set[str]:
    fn = _require("watchlist_offer_ids_from_store", missing="watchlist")
    return set(fn(_db_path(store), market_id))


def set_watched_coin_ids_for_market(
    *,
    coin_watchlist: Any,
    market_id: str,
    coin_ids: set[str],
) -> None:
    fn = _require("set_watched_coin_ids_for_market", missing="watchlist")
    fn(coin_watchlist, market_id, sorted(coin_ids))


def match_watched_coin_ids(
    *,
    coin_watchlist: Any,
    observed_coin_ids: list[str],
) -> dict[str, list[str]]:
    fn = _require("match_watched_coin_ids", missing="watchlist")
    payload = fn(coin_watchlist, observed_coin_ids)
    return {str(market_id): list(coin_ids) for market_id, coin_ids in dict(payload).items()}


def watched_coin_ids_for_market(*, coin_watchlist: Any, market_id: str) -> set[str]:
    fn = _require("watched_coin_ids_for_market", missing="watchlist")
    return set(fn(coin_watchlist, market_id))


def update_market_coin_watchlist_from_dexie(
    *,
    market: Any,
    offers: list[dict[str, Any]],
    store: SqliteStore,
    coin_watchlist: Any | None = None,
) -> None:
    cache = coin_watchlist or new_coin_watchlist_cache()
    fn = _require("update_market_coin_watchlist_from_offers", missing="watchlist")
    fn(_db_path(store), cache, market.market_id, offers)


def build_dexie_size_by_offer_id(
    offers: list[dict[str, Any]], base_asset_id: str
) -> dict[str, int]:
    fn = _require("build_dexie_size_by_offer_id", missing="watchlist")
    payload = fn(offers, base_asset_id)
    return {str(offer_id): int(size) for offer_id, size in dict(payload).items()}

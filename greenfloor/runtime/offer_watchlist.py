"""Rust-backed offer watchlist helpers (PyO3)."""

from __future__ import annotations

from datetime import datetime
from typing import Any

from greenfloor.core.cycle import is_dexie_offer_missing_error_text
from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.core.offer_lifecycle import OfferLifecycleState
from greenfloor.storage.sqlite import SqliteStore

__all__ = [
    "RESEED_MEMPOOL_MAX_AGE_SECONDS",
    "build_dexie_size_by_offer_id",
    "is_dexie_offer_missing_error",
    "is_recent_mempool_observed_offer_state",
    "match_watched_coin_ids",
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


RESEED_MEMPOOL_MAX_AGE_SECONDS = 3 * 60


def is_dexie_offer_missing_error(error: Exception) -> bool:
    return is_dexie_offer_missing_error_text(str(error))


def is_recent_mempool_observed_offer_state(
    *,
    offer_state: dict[str, Any],
    clock: datetime,
    max_age_seconds: int = RESEED_MEMPOOL_MAX_AGE_SECONDS,
) -> bool:
    state = str(offer_state.get("state", "")).strip().lower()
    if state != OfferLifecycleState.MEMPOOL_OBSERVED.value:
        return False
    updated_at_raw = str(offer_state.get("updated_at", "")).strip()
    if not updated_at_raw:
        return False
    normalized = updated_at_raw.replace("Z", "+00:00")
    try:
        updated_at = datetime.fromisoformat(normalized)
    except ValueError:
        return False
    if updated_at.tzinfo is None:
        updated_at = updated_at.replace(tzinfo=clock.tzinfo)
    age_seconds = (clock - updated_at).total_seconds()
    return 0 <= age_seconds <= float(max_age_seconds)


def watchlist_offer_ids_from_store(
    *, store: SqliteStore, market_id: str, clock: datetime | None = None
) -> set[str]:
    del clock
    fn = _require("watchlist_offer_ids_from_store", missing="watchlist")
    return set(fn(_db_path(store), market_id))


def set_watched_coin_ids_for_market(*, market_id: str, coin_ids: set[str]) -> None:
    fn = _require("set_watched_coin_ids_for_market", missing="watchlist")
    fn(market_id, sorted(coin_ids))


def match_watched_coin_ids(*, observed_coin_ids: list[str]) -> dict[str, list[str]]:
    fn = _require("match_watched_coin_ids", missing="watchlist")
    payload = fn(observed_coin_ids)
    return {str(market_id): list(coin_ids) for market_id, coin_ids in dict(payload).items()}


def watched_coin_ids_for_market(*, market_id: str) -> set[str]:
    fn = _require("watched_coin_ids_for_market", missing="watchlist")
    return set(fn(market_id))


def update_market_coin_watchlist_from_dexie(
    *,
    market: Any,
    offers: list[dict[str, Any]],
    store: SqliteStore,
    clock: datetime | None = None,
) -> None:
    del clock
    fn = _require("update_market_coin_watchlist_from_offers", missing="watchlist")
    fn(_db_path(store), market.market_id, offers)


def build_dexie_size_by_offer_id(
    offers: list[dict[str, Any]], base_asset_id: str
) -> dict[str, int]:
    fn = _require("build_dexie_size_by_offer_id", missing="watchlist")
    payload = fn(offers, base_asset_id)
    return {str(offer_id): int(size) for offer_id, size in dict(payload).items()}

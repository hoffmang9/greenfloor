"""Offer watchlist state and Dexie reconciliation helpers (shared runtime)."""

from __future__ import annotations

import logging
import threading
from datetime import UTC, datetime
from typing import Any

from greenfloor.adapters.coinset import extract_coin_ids_from_offer_payload
from greenfloor.core.cycle import is_dexie_offer_missing_error_text
from greenfloor.core.offer_lifecycle import OfferLifecycleState
from greenfloor.storage.sqlite import SqliteStore

_logger = logging.getLogger(__name__)

_RESEED_MEMPOOL_MAX_AGE_SECONDS = 3 * 60
_WATCHED_COIN_IDS_BY_MARKET: dict[str, set[str]] = {}
_WATCHED_COIN_IDS_LOCK = threading.Lock()

__all__ = [
    "build_dexie_size_by_offer_id",
    "is_dexie_offer_missing_error",
    "is_recent_mempool_observed_offer_state",
    "match_watched_coin_ids",
    "recent_executed_offer_ids",
    "set_watched_coin_ids_for_market",
    "update_market_coin_watchlist_from_dexie",
    "watched_coin_ids_for_market",
    "watchlist_offer_ids_from_store",
]


def is_recent_mempool_observed_offer_state(
    *,
    offer_state: dict[str, Any],
    clock: datetime,
    max_age_seconds: int = _RESEED_MEMPOOL_MAX_AGE_SECONDS,
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
        _logger.warning("offer state timestamp missing timezone, assuming UTC: %s", updated_at_raw)
        updated_at = updated_at.replace(tzinfo=UTC)
    age_seconds = (clock - updated_at).total_seconds()
    return 0 <= age_seconds <= float(max_age_seconds)


def is_dexie_offer_missing_error(error: Exception) -> bool:
    return is_dexie_offer_missing_error_text(str(error))


def recent_executed_offer_ids(*, store: SqliteStore, market_id: str) -> set[str]:
    events = store.list_recent_audit_events(
        event_types=["strategy_offer_execution"],
        market_id=market_id,
        limit=1500,
    )
    offer_ids: set[str] = set()
    for event in events:
        payload = event.get("payload")
        if not isinstance(payload, dict):
            continue
        single_offer_id = str(payload.get("offer_id", "")).strip()
        if single_offer_id:
            offer_ids.add(single_offer_id)
        items = payload.get("items")
        if not isinstance(items, list):
            continue
        for item in items:
            if not isinstance(item, dict):
                continue
            if str(item.get("status", "")).strip().lower() not in (
                "executed",
                "pending_visibility",
            ):
                continue
            item_offer_id = str(item.get("offer_id", "")).strip()
            if item_offer_id:
                offer_ids.add(item_offer_id)
    return offer_ids


def watchlist_offer_ids_from_store(
    *, store: SqliteStore, market_id: str, clock: datetime
) -> set[str]:
    tracked_states = {
        OfferLifecycleState.OPEN.value,
        OfferLifecycleState.REFRESH_DUE.value,
        "unknown_orphaned",
    }
    offer_ids: set[str] = set()
    for item in store.list_offer_states(market_id=market_id, limit=500):
        state = str(item.get("state", "")).strip().lower()
        offer_id = str(item.get("offer_id", "")).strip()
        if not offer_id:
            continue
        if state in tracked_states or is_recent_mempool_observed_offer_state(
            offer_state=item, clock=clock
        ):
            offer_ids.add(offer_id)
    return offer_ids


def set_watched_coin_ids_for_market(*, market_id: str, coin_ids: set[str]) -> None:
    with _WATCHED_COIN_IDS_LOCK:
        _WATCHED_COIN_IDS_BY_MARKET[market_id] = set(coin_ids)


def match_watched_coin_ids(*, observed_coin_ids: list[str]) -> dict[str, list[str]]:
    normalized = {
        str(coin_id).strip().lower() for coin_id in observed_coin_ids if str(coin_id).strip()
    }
    if not normalized:
        return {}
    matches: dict[str, list[str]] = {}
    with _WATCHED_COIN_IDS_LOCK:
        for market_id, watched in _WATCHED_COIN_IDS_BY_MARKET.items():
            intersection = sorted(normalized.intersection(watched))
            if intersection:
                matches[market_id] = intersection
    return matches


def watched_coin_ids_for_market(*, market_id: str) -> set[str]:
    with _WATCHED_COIN_IDS_LOCK:
        return set(_WATCHED_COIN_IDS_BY_MARKET.get(market_id, set()))


def update_market_coin_watchlist_from_dexie(
    *,
    market: Any,
    offers: list[dict[str, Any]],
    store: SqliteStore,
    clock: datetime,
) -> None:
    watch_offer_ids = watchlist_offer_ids_from_store(
        store=store,
        market_id=market.market_id,
        clock=clock,
    )
    watch_offer_ids.update(recent_executed_offer_ids(store=store, market_id=market.market_id))
    watched_coin_ids: set[str] = set()
    matched_offer_count = 0
    for offer in offers:
        offer_id = str(offer.get("id", "")).strip()
        if not offer_id or offer_id not in watch_offer_ids:
            continue
        matched_offer_count += 1
        watched_coin_ids.update(extract_coin_ids_from_offer_payload(offer))
    set_watched_coin_ids_for_market(market_id=market.market_id, coin_ids=watched_coin_ids)
    store.add_audit_event(
        "coin_watchlist_updated",
        {
            "market_id": market.market_id,
            "watch_offer_count": len(watch_offer_ids),
            "matched_offer_count": matched_offer_count,
            "watch_coin_count": len(watched_coin_ids),
            "watch_coin_sample": sorted(watched_coin_ids)[:10],
        },
        market_id=market.market_id,
    )


def build_dexie_size_by_offer_id(
    offers: list[dict[str, Any]], base_asset_id: str
) -> dict[str, int]:
    """Extract {offer_id -> size_base_units} from flat Dexie offer dicts."""
    result: dict[str, int] = {}
    clean_base = str(base_asset_id).strip().lower()
    for offer in offers:
        if not isinstance(offer, dict):
            continue
        offer_id = str(offer.get("id", "")).strip()
        if not offer_id:
            continue
        for offered_item in offer.get("offered") or []:
            if not isinstance(offered_item, dict):
                continue
            if str(offered_item.get("id", "")).strip().lower() != clean_base:
                continue
            try:
                size = int(offered_item["amount"])
            except (TypeError, ValueError, KeyError):
                continue
            if size > 0:
                result[offer_id] = size
    return result

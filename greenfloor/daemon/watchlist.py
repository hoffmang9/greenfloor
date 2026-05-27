"""Daemon offer watchlist, active-offer counting, and Dexie size maps."""
from __future__ import annotations
import threading
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any
from greenfloor.adapters.coinset import extract_coin_ids_from_offer_payload
from greenfloor.core.offer_lifecycle import OfferLifecycleState
from greenfloor.core.strategy import StrategyConfig
from greenfloor.daemon.cooldowns import PENDING_VISIBILITY_REASON
from greenfloor.daemon.market_helpers import _normalize_offer_side
from greenfloor.daemon.market_logging import _daemon_logger
from greenfloor.runtime.offer_publish import is_transient_dexie_visibility_404_error
from greenfloor.storage.sqlite import SqliteStore

_ACTIVE_OFFER_STATES_FOR_RESEED = {
    OfferLifecycleState.OPEN.value,
    OfferLifecycleState.REFRESH_DUE.value,
}
_RESEED_MEMPOOL_MAX_AGE_SECONDS = 3 * 60
_PENDING_VISIBILITY_RECHECK_MAX_AGE_SECONDS = 2 * 60
_WATCHED_COIN_IDS_BY_MARKET: dict[str, set[str]] = {}
_WATCHED_COIN_IDS_LOCK = threading.Lock()


@dataclass(frozen=True, slots=True)
class _OfferExecutionMetadata:
    size: int
    side: str | None
    reason: str
    created_at: str
def _is_recent_mempool_observed_offer_state(
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
        _daemon_logger.warning(
            "offer state timestamp missing timezone, assuming UTC: %s", updated_at_raw
        )
        updated_at = updated_at.replace(tzinfo=UTC)
    age_seconds = (clock - updated_at).total_seconds()
    return 0 <= age_seconds <= float(max_age_seconds)
def _strategy_target_counts_by_size(strategy_config: StrategyConfig) -> dict[int, int]:
    if strategy_config.target_counts_by_size:
        return {
            int(size): int(target)
            for size, target in sorted(strategy_config.target_counts_by_size.items())
            if int(size) > 0 and int(target) >= 0
        }
    return {
        1: int(strategy_config.ones_target),
        10: int(strategy_config.tens_target),
        100: int(strategy_config.hundreds_target),
    }
def _recent_offer_sizes_by_offer_id(*, store: SqliteStore, market_id: str) -> dict[str, int]:
    events = store.list_recent_audit_events(
        event_types=["strategy_offer_execution"],
        market_id=market_id,
        limit=1500,
    )
    size_by_offer_id: dict[str, int] = {}
    for event in events:
        payload = event.get("payload")
        if not isinstance(payload, dict):
            continue
        items = payload.get("items")
        if not isinstance(items, list):
            continue
        for item in items:
            if not isinstance(item, dict):
                continue
            if str(item.get("status", "")).strip().lower() != "executed":
                continue
            offer_id = str(item.get("offer_id", "")).strip()
            if not offer_id:
                continue
            try:
                size = int(item.get("size") or 0)
            except (TypeError, ValueError):
                continue
            if size <= 0:
                continue
            # Events are returned newest-first; keep first (latest) mapping.
            if offer_id not in size_by_offer_id:
                size_by_offer_id[offer_id] = size
    return size_by_offer_id
def _parse_offer_side_metadata(value: Any) -> str | None:
    side = str(value or "").strip().lower()
    if side in {"buy", "sell"}:
        return side
    return None
def _recent_offer_metadata_by_offer_id(
    *, store: SqliteStore, market_id: str
) -> dict[str, _OfferExecutionMetadata]:
    events = store.list_recent_audit_events(
        event_types=["strategy_offer_execution"],
        market_id=market_id,
        limit=1500,
    )
    metadata_by_offer_id: dict[str, _OfferExecutionMetadata] = {}
    for event in events:
        created_at = str(event.get("created_at", "")).strip()
        payload = event.get("payload")
        if not isinstance(payload, dict):
            continue
        items = payload.get("items")
        if not isinstance(items, list):
            continue
        for item in items:
            if not isinstance(item, dict):
                continue
            if str(item.get("status", "")).strip().lower() != "executed":
                continue
            offer_id = str(item.get("offer_id", "")).strip()
            if not offer_id:
                continue
            try:
                size = int(item.get("size") or 0)
            except (TypeError, ValueError):
                continue
            if size <= 0:
                continue
            side = _parse_offer_side_metadata(item.get("side"))
            reason = str(item.get("reason", "")).strip()
            # Events are returned newest-first; keep first (latest) mapping.
            if offer_id not in metadata_by_offer_id:
                metadata_by_offer_id[offer_id] = _OfferExecutionMetadata(
                    size=size,
                    side=side,
                    reason=reason,
                    created_at=created_at,
                )
    return metadata_by_offer_id
def _parse_event_created_at(value: Any) -> datetime | None:
    raw = str(value or "").strip()
    if not raw:
        return None
    normalized = raw.replace("Z", "+00:00")
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed
def _is_stale_pending_visibility_offer(
    *,
    offer_id: str,
    metadata: _OfferExecutionMetadata,
    dexie_size_by_offer_id: dict[str, int] | None,
    clock: datetime,
    max_age_seconds: int = _PENDING_VISIBILITY_RECHECK_MAX_AGE_SECONDS,
) -> bool:
    if metadata.reason != PENDING_VISIBILITY_REASON:
        return False
    if dexie_size_by_offer_id is None:
        # No Dexie visibility snapshot available this cycle.
        return False
    if offer_id in dexie_size_by_offer_id:
        return False
    created_at_raw = str(metadata.created_at).strip()
    if not created_at_raw:
        return True
    normalized = created_at_raw.replace("Z", "+00:00")
    try:
        created_at = datetime.fromisoformat(normalized)
    except ValueError:
        return True
    if created_at.tzinfo is None:
        created_at = created_at.replace(tzinfo=UTC)
    return (clock - created_at).total_seconds() > float(max_age_seconds)
def _is_dexie_offer_missing_error(error: Exception) -> bool:
    raw = str(error).strip()
    if not raw:
        return False
    normalized = raw.lower()
    return is_transient_dexie_visibility_404_error(raw) or (
        "http error 404" in normalized and "not found" in normalized
    )
def _recent_executed_offer_ids(*, store: SqliteStore, market_id: str) -> set[str]:
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
            if str(item.get("status", "")).strip().lower() != "executed":
                continue
            item_offer_id = str(item.get("offer_id", "")).strip()
            if item_offer_id:
                offer_ids.add(item_offer_id)
    return offer_ids
def _watchlist_offer_ids_from_store(
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
        if state in tracked_states or _is_recent_mempool_observed_offer_state(
            offer_state=item, clock=clock
        ):
            offer_ids.add(offer_id)
    return offer_ids
def _set_watched_coin_ids_for_market(*, market_id: str, coin_ids: set[str]) -> None:
    with _WATCHED_COIN_IDS_LOCK:
        _WATCHED_COIN_IDS_BY_MARKET[market_id] = set(coin_ids)
def _match_watched_coin_ids(*, observed_coin_ids: list[str]) -> dict[str, list[str]]:
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
def _watched_coin_ids_for_market(*, market_id: str) -> set[str]:
    with _WATCHED_COIN_IDS_LOCK:
        return set(_WATCHED_COIN_IDS_BY_MARKET.get(market_id, set()))
def _update_market_coin_watchlist_from_dexie(
    *,
    market,
    offers: list[dict[str, Any]],
    store: SqliteStore,
    clock: datetime,
) -> None:
    watch_offer_ids = _watchlist_offer_ids_from_store(
        store=store,
        market_id=market.market_id,
        clock=clock,
    )
    watch_offer_ids.update(_recent_executed_offer_ids(store=store, market_id=market.market_id))
    watched_coin_ids: set[str] = set()
    matched_offer_count = 0
    for offer in offers:
        offer_id = str(offer.get("id", "")).strip()
        if not offer_id or offer_id not in watch_offer_ids:
            continue
        matched_offer_count += 1
        watched_coin_ids.update(extract_coin_ids_from_offer_payload(offer))
    _set_watched_coin_ids_for_market(market_id=market.market_id, coin_ids=watched_coin_ids)
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
def _build_dexie_size_by_offer_id(
    offers: list[dict[str, Any]], base_asset_id: str
) -> dict[str, int]:
    """Extract {offer_id -> size_base_units} from a list of flat Dexie offer dicts.

    Works with both the list endpoint (each element is a flat offer dict) and a
    single element extracted from a get_offer() response (payload["offer"]).
    """
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
def _active_offer_state_summary(
    *,
    store: SqliteStore,
    market_id: str,
    clock: datetime,
    limit: int = 500,
) -> tuple[list[str], dict[str, int], dict[str, _OfferExecutionMetadata]]:
    offer_states = store.list_offer_states(market_id=market_id, limit=limit)
    state_counts: dict[str, int] = {}
    for item in offer_states:
        state = str(item.get("state", "")).strip().lower()
        if not state:
            continue
        state_counts[state] = int(state_counts.get(state, 0)) + 1
    active_offer_ids: list[str] = []
    for item in offer_states:
        state = str(item.get("state", "")).strip().lower()
        if state in _ACTIVE_OFFER_STATES_FOR_RESEED:
            active_offer_id = str(item.get("offer_id", "")).strip()
            if active_offer_id:
                active_offer_ids.append(active_offer_id)
            continue
        if _is_recent_mempool_observed_offer_state(offer_state=item, clock=clock):
            active_offer_id = str(item.get("offer_id", "")).strip()
            if active_offer_id:
                active_offer_ids.append(active_offer_id)
    return (
        active_offer_ids,
        state_counts,
        _recent_offer_metadata_by_offer_id(store=store, market_id=market_id),
    )
def _active_offer_counts_by_size(
    *,
    store: SqliteStore,
    market_id: str,
    clock: datetime,
    limit: int = 500,
    dexie_size_by_offer_id: dict[str, int] | None = None,
    tracked_sizes: set[int] | None = None,
) -> tuple[dict[int, int], dict[str, int], int]:
    active_offer_ids, state_counts, metadata_by_offer_id = _active_offer_state_summary(
        store=store,
        market_id=market_id,
        clock=clock,
        limit=limit,
    )
    normalized_sizes = (
        {int(size) for size in tracked_sizes if int(size) > 0}
        if tracked_sizes is not None
        else {1, 10, 100}
    )
    active_counts_by_size: dict[int, int] = {size: 0 for size in sorted(normalized_sizes)}
    active_unmapped_offer_ids = 0
    for offer_id in active_offer_ids:
        metadata = metadata_by_offer_id.get(offer_id)
        if metadata is not None and _is_stale_pending_visibility_offer(
            offer_id=offer_id,
            metadata=metadata,
            dexie_size_by_offer_id=dexie_size_by_offer_id,
            clock=clock,
        ):
            active_unmapped_offer_ids += 1
            continue
        size = metadata.size if metadata is not None else None
        if size is None and dexie_size_by_offer_id:
            size = dexie_size_by_offer_id.get(offer_id)
        if size in active_counts_by_size:
            active_counts_by_size[size] = int(active_counts_by_size[size]) + 1
        else:
            active_unmapped_offer_ids += 1
    return active_counts_by_size, state_counts, active_unmapped_offer_ids
def _active_offer_counts_by_size_and_side(
    *,
    store: SqliteStore,
    market_id: str,
    clock: datetime,
    limit: int = 500,
    dexie_size_by_offer_id: dict[str, int] | None = None,
    tracked_sizes: set[int] | None = None,
) -> tuple[dict[str, dict[int, int]], dict[str, int], int]:
    normalized_sizes = (
        {int(size) for size in tracked_sizes if int(size) > 0}
        if tracked_sizes is not None
        else {1, 10, 100}
    )
    counts_by_side: dict[str, dict[int, int]] = {
        "buy": {size: 0 for size in sorted(normalized_sizes)},
        "sell": {size: 0 for size in sorted(normalized_sizes)},
    }
    active_offer_ids, state_counts, metadata_by_offer_id = _active_offer_state_summary(
        store=store,
        market_id=market_id,
        clock=clock,
        limit=limit,
    )
    active_unmapped_offer_ids = 0
    for offer_id in active_offer_ids:
        metadata = metadata_by_offer_id.get(offer_id)
        if metadata is not None and _is_stale_pending_visibility_offer(
            offer_id=offer_id,
            metadata=metadata,
            dexie_size_by_offer_id=dexie_size_by_offer_id,
            clock=clock,
        ):
            active_unmapped_offer_ids += 1
            continue
        size = metadata.size if metadata is not None else None
        side = metadata.side if metadata is not None else None
        if metadata is None or side is None:
            # Do not assume buy/sell direction when metadata is unavailable.
            active_unmapped_offer_ids += 1
            continue
        if size is None and dexie_size_by_offer_id:
            size = dexie_size_by_offer_id.get(offer_id)
        normalized_side = _normalize_offer_side(side)
        if size in counts_by_side[normalized_side]:
            counts_by_side[normalized_side][size] = int(counts_by_side[normalized_side][size]) + 1
        else:
            active_unmapped_offer_ids += 1
    return counts_by_side, state_counts, active_unmapped_offer_ids

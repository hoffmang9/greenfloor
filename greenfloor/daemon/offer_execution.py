"""Daemon strategy offer execution (managed signer path)."""

from __future__ import annotations

import concurrent.futures
import threading
import time
import urllib.parse
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from greenfloor.adapters.coinset import CoinsetAdapter, extract_coin_ids_from_offer_payload
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.config.models import (
    MarketConfig,
    ProgramConfig,
    managed_offer_execution_backend,
    signer_offer_path_configured,
)
from greenfloor.core.offer_lifecycle import OfferLifecycleState
from greenfloor.core.strategy import PlannedAction, StrategyConfig, evaluate_market
from greenfloor.daemon.coinset_ws import capture_coinset_websocket_once
from greenfloor.daemon.cooldowns import (
    _POST_COOLDOWN_UNTIL,
    _cooldown_remaining_ms,
    _is_transient_managed_upstream_error_text,
    _post_offer_with_retry,
    _post_retry_config,
    _set_cooldown,
)
from greenfloor.daemon.market_helpers import _resolve_quote_asset_for_offer
from greenfloor.daemon.market_logging import (
    _daemon_logger,
    _log_market_decision,
    _log_offer_action_timing,
)
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_state import (
    _strategy_state_from_bucket_counts,
)
from greenfloor.hex_utils import default_mojo_multiplier_for_asset, is_hex_id
from greenfloor.runtime.coin_ops.coins import is_spendable_coin
from greenfloor.runtime.coinset_coins import list_unspent_coins_by_receive_address
from greenfloor.runtime.offer_build_context import (
    default_program_config_path,
    prepare_offer_build_context,
)
from greenfloor.runtime.offer_execution import build_daemon_action_offer_payload
from greenfloor.runtime.offer_post_request import OfferPostRequest, parse_managed_offer_post_result
from greenfloor.runtime.offer_publish import (
    is_transient_dexie_visibility_404_error,
    resolve_quote_price_for_market,
    verify_offer_visible_on_dexie,
)
from greenfloor.runtime.offer_runtime import signer_resolve_offer_asset_ids
from greenfloor.storage.sqlite import SqliteStore

_ACTIVE_OFFER_STATES_FOR_RESEED = {
    OfferLifecycleState.OPEN.value,
    OfferLifecycleState.REFRESH_DUE.value,
}
_RESEED_MEMPOOL_MAX_AGE_SECONDS = 3 * 60
_PENDING_VISIBILITY_RECHECK_MAX_AGE_SECONDS = 2 * 60
_PENDING_VISIBILITY_REASON = "managed_offer_post_success_dexie_visibility_pending"

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


def _normalize_offer_side(value: Any) -> str:
    side = str(value or "").strip().lower()
    return "buy" if side == "buy" else "sell"


def _action_item(
    action: Any,
    *,
    status: str,
    reason: str,
    offer_id: str | None = None,
    **extra: Any,
) -> dict[str, Any]:
    return {
        "size": action.size,
        "side": _normalize_offer_side(getattr(action, "side", "sell")),
        "status": status,
        "reason": reason,
        "offer_id": offer_id,
        **extra,
    }


def _parallel_offer_worker_error_item(*, exc: Exception) -> dict[str, Any]:
    return {
        "size": 0,
        "side": "sell",
        "status": "skipped",
        "reason": f"parallel_offer_worker_error:{exc}",
        "offer_id": None,
    }


def _can_parallelize_managed_offers(
    *,
    program: ProgramConfig | None,
    runtime_dry_run: bool,
    reservation_coordinator: AssetReservationCoordinator | None,
) -> bool:
    return (
        program is not None
        and signer_offer_path_configured(program)
        and bool(program.runtime_offer_parallelism_enabled)
        and not runtime_dry_run
        and reservation_coordinator is not None
    )


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


def _expiry_seconds_for_action(action: PlannedAction) -> int | None:
    unit = str(action.expiry_unit or "").strip().lower()
    try:
        value = int(action.expiry_value)
    except (TypeError, ValueError):
        return None
    if value <= 0:
        return None
    unit_seconds = {
        "second": 1,
        "seconds": 1,
        "minute": 60,
        "minutes": 60,
        "hour": 60 * 60,
        "hours": 60 * 60,
        "day": 24 * 60 * 60,
        "days": 24 * 60 * 60,
    }.get(unit)
    if unit_seconds is None:
        return None
    return value * unit_seconds


def _apply_action_cadence_gate(
    *,
    actions: list[PlannedAction],
    target_counts_by_side: dict[str, dict[int, int]],
    active_counts_by_side: dict[str, dict[int, int]],
    store: SqliteStore,
    market_id: str,
    clock: datetime,
) -> tuple[list[PlannedAction], list[dict[str, Any]]]:
    _ = target_counts_by_side, active_counts_by_side, store, market_id, clock
    passthrough_actions = [action for action in actions if int(action.repeat) > 0]
    return passthrough_actions, []


def _is_stale_pending_visibility_offer(
    *,
    offer_id: str,
    metadata: _OfferExecutionMetadata,
    dexie_size_by_offer_id: dict[str, int] | None,
    clock: datetime,
    max_age_seconds: int = _PENDING_VISIBILITY_RECHECK_MAX_AGE_SECONDS,
) -> bool:
    if metadata.reason != _PENDING_VISIBILITY_REASON:
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


def _inject_reseed_action_if_no_active_offers(
    *,
    strategy_actions: list[PlannedAction],
    strategy_config: StrategyConfig,
    market,
    store: SqliteStore,
    xch_price_usd: float | None,
    clock: datetime,
    dexie_size_by_offer_id: dict[str, int] | None = None,
) -> list[PlannedAction]:
    if strategy_actions:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="strategy_actions_present",
            action_count=len(strategy_actions),
        )
        return strategy_actions
    target_by_size = _strategy_target_counts_by_size(strategy_config)
    active_counts_by_size, state_counts, active_unmapped_offer_ids = _active_offer_counts_by_size(
        store=store,
        market_id=market.market_id,
        clock=clock,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
        tracked_sizes=set(target_by_size.keys()),
    )
    missing_by_size = {
        size: max(0, int(target_by_size.get(size, 0)) - int(active_counts_by_size.get(size, 0)))
        for size in target_by_size
    }
    if sum(missing_by_size.values()) <= 0:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="active_offer_targets_satisfied",
            active_states=sorted(_ACTIVE_OFFER_STATES_FOR_RESEED),
            recent_mempool_window_seconds=_RESEED_MEMPOOL_MAX_AGE_SECONDS,
            state_counts=state_counts,
            active_counts_by_size=active_counts_by_size,
            target_counts_by_size=target_by_size,
            active_unmapped_offer_ids=active_unmapped_offer_ids,
        )
        return strategy_actions

    seed_candidates = evaluate_market(
        state=_strategy_state_from_bucket_counts({}, xch_price_usd=xch_price_usd),
        config=strategy_config,
        clock=clock,
    )
    if not seed_candidates:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="no_seed_candidates",
            pair=strategy_config.pair,
            xch_price_usd=xch_price_usd,
        )
        return strategy_actions

    # Reseed one action per ladder size so the market rehydrates as 1/10/100,
    # not only the smallest denomination.
    one_per_size: dict[int, PlannedAction] = {}
    for candidate in seed_candidates:
        size = int(candidate.size)
        if size not in one_per_size:
            one_per_size[size] = candidate
    reseed_actions: list[PlannedAction] = []
    for size in sorted(one_per_size):
        missing = int(missing_by_size.get(size, 0))
        if missing <= 0:
            continue
        action = one_per_size[size]
        reseed_actions.append(
            PlannedAction(
                size=int(action.size),
                repeat=int(missing),
                pair=action.pair,
                expiry_unit=action.expiry_unit,
                expiry_value=int(action.expiry_value),
                cancel_after_create=action.cancel_after_create,
                reason="offer_size_gap_reseed",
                target_spread_bps=action.target_spread_bps,
            )
        )
    if not reseed_actions:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="missing_sizes_no_seed_template",
            missing_by_size=missing_by_size,
            candidate_sizes=sorted(one_per_size),
        )
        return strategy_actions
    reseed_actions, cadence_limited_sizes = _apply_action_cadence_gate(
        actions=reseed_actions,
        target_counts_by_side={"buy": {}, "sell": dict(target_by_size)},
        active_counts_by_side={
            "buy": {},
            "sell": {int(size): int(count) for size, count in active_counts_by_size.items()},
        },
        store=store,
        market_id=market.market_id,
        clock=clock,
    )
    if not reseed_actions:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="reseed_cadence_gate_active",
            active_counts_by_size=active_counts_by_size,
            target_counts_by_size=target_by_size,
            missing_by_size=missing_by_size,
            cadence_limited_sizes=cadence_limited_sizes,
        )
        return strategy_actions

    _log_market_decision(
        market.market_id,
        "reseed_injected",
        reason="offer_size_gap_reseed",
        sizes=[int(action.size) for action in reseed_actions],
        repeats=[int(action.repeat) for action in reseed_actions],
        action_count=sum(int(action.repeat) for action in reseed_actions),
        active_counts_by_size=active_counts_by_size,
        target_counts_by_size=target_by_size,
        missing_by_size=missing_by_size,
        pair=strategy_config.pair,
        expiry_unit=reseed_actions[0].expiry_unit,
        expiry_value=int(reseed_actions[0].expiry_value),
        cadence_limited_sizes=cadence_limited_sizes,
    )
    return reseed_actions


def _build_offer_for_action(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: Any,
    xch_price_usd: float | None,
    program_path: Path | None = None,
    keyring_yaml_path: str | None = None,
) -> dict[str, Any]:
    from greenfloor.offer_builder import build_offer

    side = _normalize_offer_side(getattr(action, "side", "sell"))
    resolved_keyring_yaml_path = keyring_yaml_path
    resolved_program_path = default_program_config_path(program, program_path)
    try:
        build_ctx = prepare_offer_build_context(
            program=program,
            market=market,
            program_path=resolved_program_path,
            network=program.app_network,
            keyring_yaml_path=resolved_keyring_yaml_path,
            action_side=side,
        )
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"offer_builder_failed:{exc}",
            "offer": None,
        }
    payload = build_daemon_action_offer_payload(
        build_ctx,
        action=action,
        xch_price_usd=xch_price_usd,
    )
    try:
        offer = build_offer(payload)
    except Exception as exc:
        return {"status": "skipped", "reason": f"offer_builder_failed:{exc}", "offer": None}
    return {"status": "executed", "reason": "offer_builder_success", "offer": offer}


def _reservation_wallet_id(program: ProgramConfig) -> str:
    vault = program.vault_config
    if vault is not None:
        launcher_id = str(vault.launcher_id).strip()
        if launcher_id:
            return launcher_id
    return "signer"


def _coinset_spendable_profiles_by_asset(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    asset_ids: set[str],
) -> dict[str, dict[str, int]]:
    receive_address = str(market.receive_address).strip()
    network = str(program.app_network).strip()
    requested_asset_ids = {str(asset_id).strip() for asset_id in asset_ids if str(asset_id).strip()}
    profiles: dict[str, dict[str, int]] = {
        asset_id: {"total": 0, "max_single": 0, "coin_count": 0, "max_single_known": 1}
        for asset_id in requested_asset_ids
    }
    if not requested_asset_ids or not receive_address:
        return profiles
    for requested_asset_id in requested_asset_ids:
        profile = profiles[requested_asset_id]
        try:
            coins = list_unspent_coins_by_receive_address(
                network=network,
                receive_address=receive_address,
                asset_id=requested_asset_id,
            )
        except Exception as exc:
            _daemon_logger.warning(
                "coinset_inventory_lookup_failed asset_id=%s error=%s",
                requested_asset_id,
                exc,
            )
            continue
        for coin in coins:
            if not isinstance(coin, dict):
                continue
            if not is_spendable_coin(coin):
                continue
            try:
                amount = int(coin.get("amount", 0))
            except (TypeError, ValueError):
                amount = 0
            if amount <= 0:
                continue
            profile["total"] += amount
            profile["coin_count"] += 1
            if amount > int(profile.get("max_single", 0)):
                profile["max_single"] = amount
    return profiles


def _coinset_spendable_base_unit_coin_amounts(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    resolved_asset_id: str,
    base_unit_mojo_multiplier: int,
) -> list[int]:
    receive_address = str(market.receive_address).strip()
    if not receive_address or not str(resolved_asset_id).strip():
        return []
    multiplier = max(1, int(base_unit_mojo_multiplier))
    try:
        coins = list_unspent_coins_by_receive_address(
            network=str(program.app_network).strip(),
            receive_address=receive_address,
            asset_id=str(resolved_asset_id).strip(),
        )
    except Exception:
        return []
    amounts_base_units: list[int] = []
    for coin in coins:
        if not isinstance(coin, dict) or not is_spendable_coin(coin):
            continue
        try:
            amount_mojos = int(coin.get("amount", 0))
        except (TypeError, ValueError):
            continue
        if amount_mojos <= 0:
            continue
        amount_base_units = amount_mojos // multiplier
        if amount_base_units > 0:
            amounts_base_units.append(amount_base_units)
    return amounts_base_units


def _base_unit_mojo_multiplier_for_market(*, market: Any) -> int:
    pricing = getattr(market, "pricing", {}) or {}
    default_multiplier = default_mojo_multiplier_for_asset(str(getattr(market, "base_asset", "")))
    try:
        multiplier = int(pricing.get("base_unit_mojo_multiplier", default_multiplier))
    except (TypeError, ValueError):
        multiplier = default_multiplier
    return max(1, multiplier)


def _coinset_cat_spendable_base_unit_coin_amounts(
    *,
    canonical_asset_id: str,
    receive_address: str,
    network: str,
    base_unit_mojo_multiplier: int,
) -> list[int]:
    asset_hex = str(canonical_asset_id).strip().lower()
    if not asset_hex or not is_hex_id(asset_hex):
        return []
    try:
        import chia_wallet_sdk as sdk  # type: ignore[import-untyped]

        address = sdk.Address.decode(str(receive_address))
        inner_puzzle_hash = bytes(address.puzzle_hash)
        asset_id_bytes = bytes.fromhex(asset_hex)
        cat_puzzle_hash = sdk.cat_puzzle_hash(asset_id_bytes, inner_puzzle_hash)
        coinset = CoinsetAdapter(network=str(network))
        records = coinset.get_coin_records_by_puzzle_hash(
            puzzle_hash_hex=f"0x{bytes(cat_puzzle_hash).hex()}",
            include_spent_coins=False,
        )
    except Exception:
        return []
    multiplier = max(1, int(base_unit_mojo_multiplier))
    amounts: list[int] = []
    for record in records:
        if not isinstance(record, dict):
            continue
        coin_payload = record.get("coin")
        if not isinstance(coin_payload, dict):
            continue
        try:
            amount_mojos = int(coin_payload.get("amount", 0))
        except (TypeError, ValueError):
            continue
        if amount_mojos <= 0:
            continue
        amount_base_units = amount_mojos // multiplier
        if amount_base_units > 0:
            amounts.append(amount_base_units)
    return amounts


def _reservation_request_for_managed_offer(
    *,
    market: Any,
    action: Any,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    fee_asset_id: str,
    fee_amount_mojos: int,
) -> dict[str, int]:
    pricing = market.pricing or {}
    base_multiplier = int(pricing.get("base_unit_mojo_multiplier", 1000))
    quote_multiplier = int(pricing.get("quote_unit_mojo_multiplier", 1000))
    base_asset_id = str(resolved_base_asset_id or "").strip()
    quote_asset_id = str(resolved_quote_asset_id or "").strip()
    if not base_asset_id or not quote_asset_id:
        return {}
    side = _normalize_offer_side(getattr(action, "side", "sell"))
    base_amount = int(action.size) * base_multiplier
    quote_amount = int(
        round(
            float(action.size)
            * float(resolve_quote_price_for_market(market))
            * float(quote_multiplier)
        )
    )
    offer_asset_id = quote_asset_id if side == "buy" else base_asset_id
    offer_amount = quote_amount if side == "buy" else base_amount
    if offer_amount <= 0:
        return {}
    request: dict[str, int] = {offer_asset_id: offer_amount}
    fee_asset = str(fee_asset_id or "").strip()
    if fee_asset and int(fee_amount_mojos) > 0:
        request[fee_asset] = int(request.get(fee_asset, 0)) + int(fee_amount_mojos)
    return request


def _resolve_signer_offer_asset_ids_for_reservation(
    *,
    program: ProgramConfig,
    market: MarketConfig,
) -> tuple[str, str, str]:
    quote_asset = _resolve_quote_asset_for_offer(
        quote_asset=str(getattr(market, "quote_asset", "")),
        network=str(getattr(program, "app_network", "mainnet")),
    )
    resolved_base_asset_id, resolved_quote_asset_id = signer_resolve_offer_asset_ids(
        program=program,
        base_asset_id=str(getattr(market, "base_asset", "")).strip(),
        quote_asset_id=str(quote_asset).strip(),
    )
    resolved_xch_asset_id, _ = signer_resolve_offer_asset_ids(
        program=program,
        base_asset_id="xch",
        quote_asset_id=str(quote_asset).strip(),
    )
    return resolved_base_asset_id, resolved_quote_asset_id, resolved_xch_asset_id


def _managed_offer_post(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    size_base_units: int,
    publish_venue: str,
    runtime_dry_run: bool,
    side: str = "sell",
    program_path: Path | None = None,
) -> dict[str, Any]:
    backend = managed_offer_execution_backend(program, size_base_units=size_base_units)
    if backend is None:
        return {
            "success": False,
            "error": "managed_offer_post_requires_signer_backend",
        }

    build_ctx = prepare_offer_build_context(
        program=program,
        market=market,
        program_path=default_program_config_path(program, program_path),
        network=program.app_network,
        action_side=side,
    )
    request = OfferPostRequest(
        build_ctx=build_ctx,
        size_base_units=size_base_units,
        repeat=1,
        publish_venue=publish_venue,
        dexie_base_url=str(program.dexie_api_base),
        splash_base_url=str(program.splash_api_base),
        drop_only=True,
        claim_rewards=False,
        dry_run=runtime_dry_run,
    )
    exit_code, payload = request.run_managed(backend)
    return parse_managed_offer_post_result(exit_code, payload)


def _resolve_coinset_ws_url(*, program, coinset_base_url: str) -> str:
    configured = str(getattr(program, "tx_block_websocket_url", "")).strip()
    if configured:
        return configured
    base_url = coinset_base_url.strip()
    if not base_url:
        if program.app_network.strip().lower() in {"testnet", "testnet11"}:
            return "wss://testnet11.api.coinset.org/ws"
        return "wss://api.coinset.org/ws"
    parsed = urllib.parse.urlparse(base_url)
    scheme = "wss" if parsed.scheme == "https" else "ws"
    host = parsed.netloc or parsed.path
    if not host:
        return "wss://api.coinset.org/ws"
    return f"{scheme}://{host}/ws"


def _build_coinset_adapter(*, program, coinset_base_url: str) -> CoinsetAdapter:
    base_url = coinset_base_url.strip() or None
    try:
        return CoinsetAdapter(base_url, network=program.app_network)
    except TypeError as exc:
        if "network" not in str(exc):
            raise
        return CoinsetAdapter(base_url)


def _run_coinset_signal_capture_once(
    *,
    program,
    coinset_base_url: str,
    store: SqliteStore,
) -> None:
    coinset = _build_coinset_adapter(program=program, coinset_base_url=coinset_base_url)
    ws_url = _resolve_coinset_ws_url(program=program, coinset_base_url=coinset_base_url)

    def _on_mempool_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return
        new_count = store.observe_mempool_tx_ids(tx_ids)
        if new_count:
            store.add_audit_event(
                "mempool_observed",
                {"new_tx_ids": new_count, "source": "coinset_websocket"},
            )

    def _on_confirmed_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return
        confirmed = store.confirm_tx_ids(tx_ids)
        store.add_audit_event(
            "tx_block_confirmed",
            {
                "tx_ids": tx_ids,
                "confirmed_count": confirmed,
                "source": "coinset_websocket",
            },
        )

    def _on_audit_event(event_type: str, payload: dict[str, Any]) -> None:
        store.add_audit_event(event_type, payload)

    def _on_observed_coin_ids(coin_ids: list[str]) -> None:
        if not coin_ids:
            return
        hits = _match_watched_coin_ids(observed_coin_ids=coin_ids)
        if not hits:
            return
        store.add_audit_event(
            "coin_watch_hit",
            {
                "coin_id_count": len(coin_ids),
                "coin_ids_sample": sorted({str(c).strip().lower() for c in coin_ids})[:10],
                "market_hits": {market_id: ids[:10] for market_id, ids in hits.items()},
                "source": "coinset_websocket",
            },
        )

    capture_coinset_websocket_once(
        ws_url=ws_url,
        reconnect_interval_seconds=program.tx_block_websocket_reconnect_interval_seconds,
        capture_window_seconds=max(1, program.tx_block_fallback_poll_interval_seconds),
        on_mempool_tx_ids=_on_mempool_tx_ids,
        on_confirmed_tx_ids=_on_confirmed_tx_ids,
        on_audit_event=_on_audit_event,
        on_observed_coin_ids=_on_observed_coin_ids,
        recovery_poll=coinset.get_all_mempool_tx_ids,
    )


def _execute_single_managed_action(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: Any,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
) -> dict[str, Any]:
    """Execute a single strategy action via the managed signer path."""
    managed_post = _managed_offer_post(
        program=program,
        market=market,
        size_base_units=int(action.size),
        publish_venue=publish_venue,
        runtime_dry_run=runtime_dry_run,
        side=_normalize_offer_side(getattr(action, "side", "sell")),
    )
    timing_fields = {
        "offer_create_ms": managed_post.get("offer_create_ms"),
        "offer_publish_ms": managed_post.get("offer_publish_ms"),
        "offer_total_ms": managed_post.get("offer_total_ms"),
        "offer_create_phase_ms": managed_post.get("offer_create_phase_ms"),
        "offer_artifact_wait_ms": managed_post.get("offer_artifact_wait_ms"),
    }
    if bool(managed_post.get("success", False)):
        managed_offer_id = str(managed_post.get("offer_id", "")).strip()
        if publish_venue == "dexie" and managed_offer_id:
            visible, visibility_error = verify_offer_visible_on_dexie(
                dexie=dexie,
                offer_id=managed_offer_id,
            )
            if not visible:
                if is_transient_dexie_visibility_404_error(visibility_error or ""):
                    return _action_item(
                        action,
                        status="executed",
                        reason=_PENDING_VISIBILITY_REASON,
                        offer_id=managed_offer_id or None,
                        **timing_fields,
                    )
                return _action_item(
                    action,
                    status="skipped",
                    reason=f"managed_offer_post_not_visible_on_dexie:{visibility_error}",
                    offer_id=managed_offer_id or None,
                    **timing_fields,
                )
        return _action_item(
            action,
            status="executed",
            reason="managed_offer_post_success",
            offer_id=managed_offer_id or None,
            **timing_fields,
        )
    return _action_item(
        action,
        status="skipped",
        reason=(f"managed_offer_post_failed:{str(managed_post.get('error', 'unknown')).strip()}"),
        offer_id=None,
        **timing_fields,
    )


def _execute_managed_action_with_retry(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: Any,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
) -> dict[str, Any]:
    """Execute a single managed action with transient-error retries."""
    attempts_max, backoff_ms, _ = _post_retry_config()
    last_exc: Exception | None = None
    for attempt_index in range(max(1, int(attempts_max))):
        try:
            return _execute_single_managed_action(
                program=program,
                market=market,
                action=action,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
            )
        except Exception as exc:
            last_exc = exc
            if attempt_index >= (
                max(1, int(attempts_max)) - 1
            ) or not _is_transient_managed_upstream_error_text(str(exc)):
                raise
            if backoff_ms > 0:
                sleep_seconds = (backoff_ms * (2**attempt_index)) / 1000.0
                time.sleep(float(sleep_seconds))
    raise RuntimeError(str(last_exc or "managed_action_retry_exhausted"))


def _execute_single_local_action(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: Any,
    xch_price_usd: float | None,
    keyring_yaml_path: str,
    dexie: DexieAdapter,
    splash: SplashAdapter | None,
    publish_venue: str,
    store: SqliteStore,
    program_path: Path | None = None,
) -> dict[str, Any]:
    """Execute a single strategy action via the local build+sign+post path."""
    action_started = time.monotonic()
    build_started = action_started
    built = _build_offer_for_action(
        program=program,
        market=market,
        action=action,
        xch_price_usd=xch_price_usd,
        program_path=program_path,
        keyring_yaml_path=keyring_yaml_path,
    )
    build_ms = int((time.monotonic() - build_started) * 1000)
    if built.get("status") != "executed":
        built_reason = str(built.get("reason", "offer_builder_skipped"))
        return _action_item(
            action,
            status="skipped",
            reason=built_reason,
            offer_id=None,
            offer_create_ms=build_ms,
            offer_publish_ms=None,
            offer_total_ms=int((time.monotonic() - action_started) * 1000),
        )
    _, _, cooldown_seconds = _post_retry_config()
    cooldown_key = f"{publish_venue}:{market.market_id}"
    remaining_ms = _cooldown_remaining_ms(_POST_COOLDOWN_UNTIL, cooldown_key)
    if remaining_ms > 0:
        return _action_item(
            action,
            status="skipped",
            reason=f"post_cooldown_active:{remaining_ms}ms",
            offer_id=None,
            offer_create_ms=build_ms,
            offer_publish_ms=None,
            offer_total_ms=int((time.monotonic() - action_started) * 1000),
        )
    offer_text = str(built["offer"])
    publish_started = time.monotonic()
    post_result, attempt_count, post_error = _post_offer_with_retry(
        publish_venue=publish_venue,
        offer_text=offer_text,
        dexie=dexie,
        splash=splash,
    )
    publish_ms = int((time.monotonic() - publish_started) * 1000)
    success = bool(post_result.get("success", False))
    offer_id_raw = post_result.get("id")
    offer_id = str(offer_id_raw).strip() if offer_id_raw is not None else ""
    if success and offer_id:
        store.upsert_offer_state(
            offer_id=offer_id,
            market_id=market.market_id,
            state=OfferLifecycleState.OPEN.value,
            last_seen_status=0,
        )
        return _action_item(
            action,
            status="executed",
            reason=f"{publish_venue}_post_success",
            offer_id=offer_id,
            attempts=attempt_count,
            offer_create_ms=build_ms,
            offer_publish_ms=publish_ms,
            offer_total_ms=int((time.monotonic() - action_started) * 1000),
        )
    _set_cooldown(_POST_COOLDOWN_UNTIL, cooldown_key, cooldown_seconds)
    return _action_item(
        action,
        status="skipped",
        reason=f"{publish_venue}_post_retry_exhausted:{post_error}",
        offer_id=offer_id or None,
        attempts=attempt_count,
        offer_create_ms=build_ms,
        offer_publish_ms=publish_ms,
        offer_total_ms=int((time.monotonic() - action_started) * 1000),
    )


def _expand_strategy_actions(strategy_actions: list[Any]) -> list[Any]:
    expanded_actions: list[Any] = []
    for action in strategy_actions:
        expanded_actions.extend(action for _ in range(int(action.repeat)))
    return expanded_actions


def _managed_skip_item(*, action: Any, reason: str) -> dict[str, Any]:
    return _action_item(action, status="skipped", reason=reason, offer_id=None)


def _single_input_preferred_skip_reason(
    *,
    requested_amounts: dict[str, int],
    spendable_profiles: dict[str, dict[str, int]],
) -> str | None:
    # Prefer single-input offers on our side: if aggregate balance is
    # sufficient but no single spendable coin can satisfy the offered
    # amount, defer posting and let coin-ops combine first.
    primary_request_candidates = [
        (asset_id, int(amount))
        for asset_id, amount in requested_amounts.items()
        if str(asset_id).strip() and int(amount) > 0
    ]
    if not primary_request_candidates:
        return None
    primary_asset_id, primary_needed = max(
        primary_request_candidates, key=lambda pair: int(pair[1])
    )
    primary_profile = spendable_profiles.get(str(primary_asset_id), {})
    primary_total = int(primary_profile.get("total", 0))
    primary_max = int(primary_profile.get("max_single", 0))
    primary_max_known = bool(int(primary_profile.get("max_single_known", 0)))
    if not primary_max_known:
        return None
    if primary_total >= primary_needed and primary_max < primary_needed:
        return (
            "single_input_preferred_requires_combine"
            f":asset_id={primary_asset_id}"
            f":needed={primary_needed}"
            f":max_single={primary_max}"
            f":available={primary_total}"
        )
    return None


def _prepare_parallel_managed_submission(
    *,
    market: Any,
    action: Any,
    program: ProgramConfig,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    resolved_xch_asset_id: str,
    fee_amount_mojos: int,
) -> tuple[dict[str, int] | None, dict[str, int] | None, dict[str, Any] | None]:
    requested_amounts = _reservation_request_for_managed_offer(
        market=market,
        action=action,
        resolved_base_asset_id=resolved_base_asset_id,
        resolved_quote_asset_id=resolved_quote_asset_id,
        fee_asset_id=resolved_xch_asset_id,
        fee_amount_mojos=fee_amount_mojos,
    )
    if not requested_amounts:
        return (
            None,
            None,
            _managed_skip_item(action=action, reason="reservation_invalid_request"),
        )
    spendable_profiles = _coinset_spendable_profiles_by_asset(
        program=program,
        market=market,
        asset_ids=set(requested_amounts.keys()),
    )
    available_amounts = {
        asset_id: int(profile.get("total", 0)) for asset_id, profile in spendable_profiles.items()
    }
    single_input_skip_reason = _single_input_preferred_skip_reason(
        requested_amounts=requested_amounts,
        spendable_profiles=spendable_profiles,
    )
    if single_input_skip_reason:
        return None, None, _managed_skip_item(action=action, reason=single_input_skip_reason)
    return requested_amounts, available_amounts, None


def _strategy_action_result(
    *,
    planned_count: int,
    executed_count: int,
    items: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "planned_count": planned_count,
        "executed_count": executed_count,
        "items": items,
    }


def _execute_actions_parallel(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    expanded_actions: list[Any],
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
    reservation_coordinator: AssetReservationCoordinator,
) -> dict[str, Any]:
    items: list[dict[str, Any]] = []
    executed_count = 0
    resolved_base_asset_id, resolved_quote_asset_id, resolved_xch_asset_id = (
        _resolve_signer_offer_asset_ids_for_reservation(
            program=program,
            market=market,
        )
    )
    # Offer files must always use zero fees; fees apply only to coin split/combine.
    fee_amount_mojos = 0
    wallet_id = _reservation_wallet_id(program)
    reservation_coordinator.try_acquire(
        market_id=str(market.market_id),
        wallet_id=wallet_id,
        requested_amounts={},
        available_amounts={},
    )
    submissions: list[tuple[int, Any, dict[str, int], dict[str, int]]] = []
    for submit_index, action in enumerate(expanded_actions):
        requested_amounts, available_amounts, skip_item = _prepare_parallel_managed_submission(
            market=market,
            action=action,
            program=program,
            resolved_base_asset_id=resolved_base_asset_id,
            resolved_quote_asset_id=resolved_quote_asset_id,
            resolved_xch_asset_id=resolved_xch_asset_id,
            fee_amount_mojos=fee_amount_mojos,
        )
        if skip_item is not None:
            items.append(skip_item)
            continue
        assert requested_amounts is not None
        assert available_amounts is not None
        submissions.append((submit_index, action, requested_amounts, available_amounts))

    if not submissions:
        return _strategy_action_result(
            planned_count=len(expanded_actions),
            executed_count=executed_count,
            items=items,
        )

    max_workers = min(
        len(submissions),
        max(1, int(program.runtime_offer_parallelism_max_workers)),
    )
    _log_market_decision(
        str(market.market_id),
        "parallel_offer_dispatch",
        planned_count=len(expanded_actions),
        queued_count=len(submissions),
        workers=max_workers,
    )

    def _run_parallel_submission(
        *,
        submit_index: int,
        action: Any,
        requested_amounts: dict[str, int],
        available_amounts: dict[str, int],
        queued_at_monotonic: float,
    ) -> dict[str, Any]:
        queue_wait_ms = int((time.monotonic() - queued_at_monotonic) * 1000)
        _log_market_decision(
            str(market.market_id),
            "parallel_offer_queue_wait",
            submit_index=submit_index,
            size=int(getattr(action, "size", 0)),
            side=_normalize_offer_side(getattr(action, "side", "sell")),
            queue_wait_ms=queue_wait_ms,
        )
        acquire_started = time.monotonic()
        acquired = reservation_coordinator.try_acquire(
            market_id=str(market.market_id),
            wallet_id=wallet_id,
            requested_amounts=requested_amounts,
            available_amounts=available_amounts,
        )
        acquire_ms = int((time.monotonic() - acquire_started) * 1000)
        if not acquired.ok or not acquired.reservation_id:
            return {
                **_managed_skip_item(
                    action=action,
                    reason=str(acquired.error or "reservation_rejected"),
                ),
                "queue_wait_ms": queue_wait_ms,
                "reservation_acquire_ms": acquire_ms,
            }
        reservation_id = str(acquired.reservation_id)
        reserved_at = time.monotonic()
        _log_market_decision(
            str(market.market_id),
            "parallel_offer_reservation_acquired",
            submit_index=submit_index,
            reservation_id=reservation_id,
            queue_wait_ms=queue_wait_ms,
            reservation_acquire_ms=acquire_ms,
        )
        try:
            item = _execute_managed_action_with_retry(
                program=program,
                market=market,
                action=action,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
            )
        except Exception as exc:
            item = _parallel_offer_worker_error_item(exc=exc)
        release_status = (
            "released_success"
            if str(item.get("status", "")).strip().lower() == "executed"
            else "released_failed"
        )
        reservation_coordinator.release(reservation_id=reservation_id, status=release_status)
        reservation_hold_ms = int((time.monotonic() - reserved_at) * 1000)
        _log_market_decision(
            str(market.market_id),
            "parallel_offer_reservation_released",
            submit_index=submit_index,
            reservation_id=reservation_id,
            release_status=release_status,
            reservation_hold_ms=reservation_hold_ms,
        )
        item["reservation_id"] = reservation_id
        item["queue_wait_ms"] = queue_wait_ms
        item["reservation_acquire_ms"] = acquire_ms
        item["reservation_hold_ms"] = reservation_hold_ms
        return item

    with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
        future_to_submission: dict[concurrent.futures.Future[dict[str, Any]], int] = {}
        for submit_index, action, requested_amounts, available_amounts in submissions:
            future = pool.submit(
                _run_parallel_submission,
                submit_index=submit_index,
                action=action,
                requested_amounts=requested_amounts,
                available_amounts=available_amounts,
                queued_at_monotonic=time.monotonic(),
            )
            future_to_submission[future] = submit_index
        submitted_items: list[tuple[int, dict[str, Any]]] = []
        for future in concurrent.futures.as_completed(future_to_submission):
            submit_index = future_to_submission[future]
            try:
                item = future.result()
            except Exception as exc:
                item = _parallel_offer_worker_error_item(exc=exc)
            submitted_items.append((submit_index, item))
        for _, item in sorted(submitted_items, key=lambda pair: pair[0]):
            _log_offer_action_timing(str(market.market_id), item)
            if item.get("status") == "executed":
                executed_count += 1
            items.append(item)

    _, _, cooldown_seconds = _post_retry_config()
    transient_parallel_failures = sum(
        1
        for _submit_idx, item in submitted_items
        if str(item.get("status", "")).strip().lower() == "skipped"
        and _is_transient_managed_upstream_error_text(str(item.get("reason", "")))
    )
    total_parallel = len(submitted_items)
    if (
        total_parallel > 0
        and cooldown_seconds > 0
        and transient_parallel_failures >= max(2, (total_parallel + 1) // 2)
    ):
        cooldown_key = f"{publish_venue}:{market.market_id}"
        _set_cooldown(_POST_COOLDOWN_UNTIL, cooldown_key, cooldown_seconds)
        _log_market_decision(
            str(market.market_id),
            "parallel_offer_transient_cooldown",
            transient_failures=transient_parallel_failures,
            total_parallel=total_parallel,
            cooldown_seconds=cooldown_seconds,
        )
    return _strategy_action_result(
        planned_count=len(expanded_actions),
        executed_count=executed_count,
        items=items,
    )


def _execute_actions_sequential(
    *,
    program: ProgramConfig | None,
    market: MarketConfig,
    expanded_actions: list[Any],
    runtime_dry_run: bool,
    xch_price_usd: float | None,
    dexie: DexieAdapter,
    splash: SplashAdapter | None,
    publish_venue: str,
    store: SqliteStore,
    keyring_yaml_path: str,
) -> dict[str, Any]:
    items: list[dict[str, Any]] = []
    executed_count = 0
    for action in expanded_actions:
        if runtime_dry_run:
            items.append(_action_item(action, status="planned", reason="dry_run", offer_id=None))
            continue
        backend = (
            managed_offer_execution_backend(program, size_base_units=int(action.size))
            if program is not None
            else None
        )
        if backend is not None:
            assert program is not None
            try:
                item = _execute_managed_action_with_retry(
                    program=program,
                    market=market,
                    action=action,
                    publish_venue=publish_venue,
                    runtime_dry_run=runtime_dry_run,
                    dexie=dexie,
                )
            except Exception as exc:
                item = _action_item(
                    action,
                    status="skipped",
                    reason=f"managed_action_error:{exc}",
                    offer_id=None,
                )
        elif program is None:
            item = _action_item(
                action,
                status="skipped",
                reason="local_offer_post_requires_program_config",
                offer_id=None,
            )
        else:
            item = _execute_single_local_action(
                program=program,
                market=market,
                action=action,
                xch_price_usd=xch_price_usd,
                keyring_yaml_path=keyring_yaml_path,
                dexie=dexie,
                splash=splash,
                publish_venue=publish_venue,
                store=store,
            )
        if item.get("status") == "executed":
            executed_count += 1
        _log_offer_action_timing(str(market.market_id), item)
        items.append(item)
    return _strategy_action_result(
        planned_count=len(expanded_actions),
        executed_count=executed_count,
        items=items,
    )


def _execute_strategy_actions(
    *,
    market: MarketConfig,
    strategy_actions: list[PlannedAction],
    runtime_dry_run: bool,
    xch_price_usd: float | None,
    dexie: DexieAdapter,
    splash: SplashAdapter | None = None,
    publish_venue: str = "dexie",
    store: SqliteStore,
    app_network: str = "mainnet",
    signer_key_registry: dict[str, Any] | None = None,
    program: ProgramConfig | None = None,
    reservation_coordinator: AssetReservationCoordinator | None = None,
) -> dict[str, Any]:
    _ = app_network
    signer_key_id = str(market.signer_key_id or "").strip()
    signer_key = (signer_key_registry or {}).get(signer_key_id)
    if isinstance(signer_key, dict):
        keyring_yaml_path = str(signer_key.get("keyring_yaml_path", "") or "").strip()
    else:
        keyring_yaml_path = str(getattr(signer_key, "keyring_yaml_path", "") or "").strip()
    expanded_actions = _expand_strategy_actions(strategy_actions)
    if _can_parallelize_managed_offers(
        program=program,
        runtime_dry_run=runtime_dry_run,
        reservation_coordinator=reservation_coordinator,
    ):
        assert program is not None
        assert reservation_coordinator is not None
        try:
            return _execute_actions_parallel(
                program=program,
                market=market,
                expanded_actions=expanded_actions,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
                reservation_coordinator=reservation_coordinator,
            )
        except Exception as exc:
            store.add_audit_event(
                "offer_parallel_fallback",
                {
                    "market_id": str(market.market_id),
                    "error": str(exc),
                    "reason": "reservation_parallel_path_failed",
                },
                market_id=str(market.market_id),
            )
    return _execute_actions_sequential(
        program=program,
        market=market,
        expanded_actions=expanded_actions,
        runtime_dry_run=runtime_dry_run,
        xch_price_usd=xch_price_usd,
        dexie=dexie,
        splash=splash,
        publish_venue=publish_venue,
        store=store,
        keyring_yaml_path=keyring_yaml_path,
    )

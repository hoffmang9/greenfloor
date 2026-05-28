"""Market batch selection and disabled-market throttling for daemon cycles."""

from __future__ import annotations

import time
from collections import deque
from dataclasses import dataclass, field
from typing import Any

from greenfloor.core.cycle import (
    enqueue_immediate_requeue,
    next_disabled_market_log_deadline,
    select_market_batch as select_market_batch_kernel,
)
from greenfloor.core.cycle import (
    should_log_disabled_market as should_log_disabled_market_kernel,
)
from greenfloor.daemon.cooldowns import _env_int
from greenfloor.daemon.market_logging import _daemon_logger, _log_market_decision

_DISABLED_MARKET_LOG_INTERVAL_SECONDS_DEFAULT = 3600
_DISABLED_MARKET_NEXT_LOG_AT: dict[str, float] = {}
_DISABLED_MARKET_STARTUP_LOGGED = False


def disabled_market_log_interval_seconds() -> int:
    return _env_int(
        "GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS",
        _DISABLED_MARKET_LOG_INTERVAL_SECONDS_DEFAULT,
        minimum=60,
    )


def should_log_disabled_market(*, market_id: str, now_monotonic: float | None = None) -> bool:
    now_value = time.monotonic() if now_monotonic is None else float(now_monotonic)
    deadline = float(_DISABLED_MARKET_NEXT_LOG_AT.get(market_id, 0.0))
    if not should_log_disabled_market_kernel(
        now_monotonic=now_value,
        next_log_deadline=deadline,
    ):
        return False
    _DISABLED_MARKET_NEXT_LOG_AT[market_id] = next_disabled_market_log_deadline(
        now_monotonic=now_value,
        interval_seconds=disabled_market_log_interval_seconds(),
    )
    return True


def log_disabled_markets_startup_once(*, markets: list[Any]) -> None:
    global _DISABLED_MARKET_STARTUP_LOGGED
    if _DISABLED_MARKET_STARTUP_LOGGED:
        return
    interval_seconds = disabled_market_log_interval_seconds()
    disabled_market_ids = [
        str(getattr(market, "market_id", "")).strip()
        for market in markets
        if not bool(getattr(market, "enabled", True))
    ]
    disabled_market_ids = [market_id for market_id in disabled_market_ids if market_id]
    if disabled_market_ids:
        _daemon_logger.info(
            "disabled_markets_startup count=%s interval_seconds=%s market_ids=%s",
            len(disabled_market_ids),
            interval_seconds,
            sorted(disabled_market_ids),
        )
        now_value = time.monotonic()
        for market_id in disabled_market_ids:
            _DISABLED_MARKET_NEXT_LOG_AT[market_id] = now_value + float(interval_seconds)
    _DISABLED_MARKET_STARTUP_LOGGED = True


def clear_disabled_market_log_state(*, market_id: str) -> None:
    _DISABLED_MARKET_NEXT_LOG_AT.pop(market_id, None)


def log_disabled_market_skip(*, market_id: str) -> None:
    if should_log_disabled_market(market_id=market_id):
        _log_market_decision(market_id, "market_skipped", reason="disabled")


@dataclass(slots=True)
class MarketDispatchState:
    cursor: int = 0
    immediate_requeue_ids: deque[str] = field(default_factory=deque)


def enqueue_immediate_requeue_market(dispatch_state: MarketDispatchState, market_id: str) -> None:
    dispatch_state.immediate_requeue_ids = deque(
        enqueue_immediate_requeue(list(dispatch_state.immediate_requeue_ids), market_id)
    )


def select_market_batch(
    *,
    enabled_markets: list[Any],
    slot_count: int,
    dispatch_state: MarketDispatchState,
) -> tuple[list[Any], list[str]]:
    enabled_by_id: dict[str, Any] = {
        str(getattr(market, "market_id", "")).strip(): market for market in enabled_markets
    }
    enabled_ids = [market_id for market_id in enabled_by_id if market_id]
    if not enabled_ids:
        dispatch_state.immediate_requeue_ids = deque()
        dispatch_state.cursor = 0
        return [], []

    selection = select_market_batch_kernel(
        enabled_market_ids=enabled_ids,
        slot_count=int(slot_count),
        cursor=int(dispatch_state.cursor),
        immediate_requeue_ids=list(dispatch_state.immediate_requeue_ids),
    )
    dispatch_state.cursor = int(selection.cursor)
    dispatch_state.immediate_requeue_ids = deque(
        str(market_id) for market_id in selection.immediate_requeue_ids if str(market_id).strip()
    )
    selected_markets = [
        enabled_by_id[str(market_id)]
        for market_id in selection.selected_market_ids
        if str(market_id).strip() in enabled_by_id
    ]
    consumed = [
        str(market_id)
        for market_id in selection.consumed_immediate_requeues
        if str(market_id).strip()
    ]
    return selected_markets, consumed

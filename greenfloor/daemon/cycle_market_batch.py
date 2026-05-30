"""Market batch selection state for daemon cycles (policy lives in Rust)."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass, field
from typing import Any

from greenfloor.core.cycle import (
    enqueue_immediate_requeue,
)
from greenfloor.core.cycle import (
    select_market_batch as select_market_batch_kernel,
)


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

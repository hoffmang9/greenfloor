"""Strategy action result item shaping."""

from __future__ import annotations

from typing import Any

from greenfloor.core.cycle import is_managed_worker_transient_error
from greenfloor.core.managed_action_outcome import ManagedActionOutcome
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.market_helpers import _normalize_offer_side
from greenfloor.daemon.strategy_action_item import StrategyActionItem

__all__ = [
    "action_item",
    "action_item_from_managed_outcome",
    "managed_skip_item",
    "parallel_offer_worker_error_item",
]


def action_item(
    action: PlannedAction,
    *,
    status: str,
    reason: str,
    offer_id: str | None = None,
    **extra: Any,
) -> StrategyActionItem:
    transient_upstream = bool(extra.pop("transient_upstream", False))
    return StrategyActionItem.from_action(
        action,
        status=status,
        reason=reason,
        side=_normalize_offer_side(action.side),
        offer_id=offer_id,
        transient_upstream=transient_upstream,
        **extra,
    )


def parallel_offer_worker_error_item(
    *, action: PlannedAction, exc: Exception
) -> StrategyActionItem:
    return StrategyActionItem.from_worker_error(
        action=action,
        exc=exc,
        transient_upstream=is_managed_worker_transient_error(exc),
    )


def action_item_from_managed_outcome(
    action: PlannedAction,
    outcome: ManagedActionOutcome,
    *,
    offer_id: str | None = None,
    **extra: Any,
) -> StrategyActionItem:
    resolved_offer_id = offer_id if offer_id is not None else outcome.offer_id
    return action_item(
        action,
        status=outcome.status,
        reason=outcome.reason,
        offer_id=resolved_offer_id,
        transient_upstream=outcome.transient_upstream,
        **extra,
    )


def managed_skip_item(*, action: PlannedAction, reason: str) -> StrategyActionItem:
    return action_item(action, status="skipped", reason=reason, offer_id=None)

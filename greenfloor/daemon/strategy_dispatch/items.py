"""Strategy action result item shaping."""

from __future__ import annotations

from typing import Any

from greenfloor.core.cycle import is_managed_worker_transient_error
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.market_helpers import _normalize_offer_side
from greenfloor.daemon.strategy_action_item import StrategyActionItem


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


def parallel_offer_worker_error_item(*, exc: Exception) -> StrategyActionItem:
    return StrategyActionItem.from_worker_error(
        exc=exc,
        transient_upstream=is_managed_worker_transient_error(exc),
    )


def action_item_from_managed_outcome(
    action: PlannedAction,
    outcome: dict[str, Any],
    *,
    offer_id: str | None = None,
    **extra: Any,
) -> StrategyActionItem:
    resolved_offer_id = offer_id
    if resolved_offer_id is None:
        raw_offer_id = outcome.get("offer_id")
        resolved_offer_id = str(raw_offer_id).strip() if raw_offer_id else None
    return action_item(
        action,
        status=str(outcome["status"]),
        reason=str(outcome["reason"]),
        offer_id=resolved_offer_id or None,
        transient_upstream=bool(outcome.get("transient_upstream", False)),
        **extra,
    )


def managed_skip_item(*, action: PlannedAction, reason: str) -> StrategyActionItem:
    return action_item(action, status="skipped", reason=reason, offer_id=None)


def strategy_action_result(
    *,
    planned_count: int,
    executed_count: int,
    items: list[StrategyActionItem],
) -> dict[str, Any]:
    return {
        "planned_count": planned_count,
        "executed_count": executed_count,
        "items": [item.to_audit_dict() for item in items],
    }

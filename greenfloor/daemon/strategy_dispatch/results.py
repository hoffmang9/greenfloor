"""Typed strategy dispatch results."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from greenfloor.daemon.strategy_action_item import StrategyActionItem


@dataclass(frozen=True, slots=True)
class StrategyActionResult:
    planned_count: int
    executed_count: int
    action_items: list[StrategyActionItem]

    @property
    def items(self) -> list[dict[str, Any]]:
        return [item.to_audit_dict() for item in self.action_items]

    def __getitem__(self, key: str) -> Any:
        if key == "planned_count":
            return self.planned_count
        if key == "executed_count":
            return self.executed_count
        if key == "items":
            return self.items
        raise KeyError(key)


def strategy_action_result(
    *,
    planned_count: int,
    executed_count: int,
    items: list[StrategyActionItem],
) -> StrategyActionResult:
    return StrategyActionResult(
        planned_count=planned_count,
        executed_count=executed_count,
        action_items=list(items),
    )

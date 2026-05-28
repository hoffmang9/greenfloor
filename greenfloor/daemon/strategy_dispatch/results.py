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

    @classmethod
    def from_items(
        cls,
        *,
        planned_count: int,
        action_items: list[StrategyActionItem],
    ) -> StrategyActionResult:
        items = list(action_items)
        executed_count = sum(1 for item in items if item.counts_as_executed)
        return cls(
            planned_count=planned_count,
            executed_count=executed_count,
            action_items=items,
        )

"""Shared test-only helpers exported from the testing package."""

from __future__ import annotations

from greenfloor.core.strategy import PlannedAction

__all__ = ["PlannedAction", "drop_zero_repeat_strategy_actions"]


def drop_zero_repeat_strategy_actions(actions: list[PlannedAction]) -> list[PlannedAction]:
    return [action for action in actions if int(action.repeat) > 0]

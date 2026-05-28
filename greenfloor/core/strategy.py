from __future__ import annotations

from datetime import datetime

from greenfloor.core.cycle._bridge import evaluate_market as evaluate_market_rust
from greenfloor.core.planned_action import (
    PlannedAction,
    planned_action_from_rust_dict,
    planned_action_from_signer_item,
    planned_actions_from_signer_list,
)
from greenfloor.core.strategy_types import MarketState, StrategyConfig

__all__ = [
    "MarketState",
    "PlannedAction",
    "StrategyConfig",
    "evaluate_market",
    "planned_action_from_rust_dict",
    "planned_action_from_signer_item",
    "planned_actions_from_signer_list",
]


def evaluate_market(
    state: MarketState,
    config: StrategyConfig,
    clock: datetime,
) -> list[PlannedAction]:
    _ = clock
    return evaluate_market_rust(state=state, config=config)

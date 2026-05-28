from __future__ import annotations

from greenfloor.core.cycle._bridge_orchestration import evaluate_market as evaluate_market_rust
from greenfloor.core.planned_action import PlannedAction, planned_actions_from_signer_list
from greenfloor.core.strategy_types import MarketState, StrategyConfig

__all__ = [
    "MarketState",
    "PlannedAction",
    "StrategyConfig",
    "evaluate_market",
    "planned_actions_from_signer_list",
]


def evaluate_market(
    state: MarketState,
    config: StrategyConfig,
) -> list[PlannedAction]:
    return evaluate_market_rust(state=state, config=config)

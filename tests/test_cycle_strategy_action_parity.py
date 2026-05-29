"""Cycle strategy-action counting parity tests."""

from __future__ import annotations

from greenfloor.core.cycle.policy import executed_sell_offer_counts_by_size
from greenfloor.core.strategy_action_item import StrategyActionItem


def test_executed_sell_offer_counts_by_size_counts_only_executed_sell_items() -> None:
    counts = executed_sell_offer_counts_by_size(
        action_items=[
            StrategyActionItem(size=10, side="sell", status="executed", reason="ok"),
            StrategyActionItem(size=10, side="sell", status="executed", reason="ok"),
            StrategyActionItem(size=10, side="buy", status="executed", reason="ok"),
            StrategyActionItem(size=10, side="sell", status="skipped", reason="skip"),
            StrategyActionItem(size=1, side="sell", status="executed", reason="ok"),
        ]
    )
    assert counts == {10: 2, 1: 1}


def test_executed_sell_offer_counts_by_size_includes_pending_visibility_sells() -> None:
    counts = executed_sell_offer_counts_by_size(
        action_items=[
            StrategyActionItem(
                size=10,
                side="sell",
                status="pending_visibility",
                reason="managed_offer_post_success",
                offer_id="offer-pending",
            ),
            StrategyActionItem(size=10, side="sell", status="skipped", reason="skip"),
        ]
    )
    assert counts == {10: 1}

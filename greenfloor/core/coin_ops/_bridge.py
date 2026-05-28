"""Internal bridge helpers for coin-op policy (Rust-backed)."""

from __future__ import annotations

from greenfloor.core.coin_ops.types import CoinOpPlan


def _require_coin_op_plans(value: object) -> list[CoinOpPlan]:
    if not isinstance(value, list):
        raise TypeError("kernel returned non-list result")
    plans: list[CoinOpPlan] = []
    for item in value:
        if not isinstance(item, CoinOpPlan):
            raise TypeError("kernel returned non-CoinOpPlan result")
        plans.append(item)
    return plans

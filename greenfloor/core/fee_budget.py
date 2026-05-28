"""Backward-compatible re-export; prefer ``greenfloor.core.coin_ops``.

Deprecated: remove after step 11 (coin-op selection/planning Rust migration) when
call sites import from ``greenfloor.core.coin_ops`` directly.
"""

from greenfloor.core.coin_ops.fee_budget import (
    fee_budget_allows_execution,
    partition_plans_by_budget,
    projected_coin_ops_fee_mojos,
)

__all__ = [
    "fee_budget_allows_execution",
    "partition_plans_by_budget",
    "projected_coin_ops_fee_mojos",
]

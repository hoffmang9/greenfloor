"""Backward-compatible re-export; prefer ``greenfloor.core.coin_ops``."""

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

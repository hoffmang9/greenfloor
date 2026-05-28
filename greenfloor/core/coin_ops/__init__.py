"""Coin-operation deterministic policy (Rust-backed kernel)."""

from greenfloor.core.coin_ops.fee_budget import (
    fee_budget_allows_execution,
    partition_plans_by_budget,
    projected_coin_ops_fee_mojos,
)
from greenfloor.core.coin_ops.inventory import compute_bucket_counts_from_coins
from greenfloor.core.coin_ops.plan import plan_coin_ops
from greenfloor.core.coin_ops.policy import (
    coin_meets_coin_op_min_amount,
    coin_op_min_amount_mojos,
    coin_op_target_amount_allowed,
)
from greenfloor.core.coin_ops.types import BucketSpec, CoinOpPlan

__all__ = [
    "BucketSpec",
    "CoinOpPlan",
    "coin_meets_coin_op_min_amount",
    "coin_op_min_amount_mojos",
    "coin_op_target_amount_allowed",
    "compute_bucket_counts_from_coins",
    "fee_budget_allows_execution",
    "partition_plans_by_budget",
    "plan_coin_ops",
    "projected_coin_ops_fee_mojos",
]

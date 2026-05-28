"""Coin-operation deterministic policy (Rust-backed kernel).

``SplitPlanningProfile`` controls ``plan_auto_split_selection()``:

- ``CLI_AUTO``: largest spendable coin; no required-amount enforcement; no sub-CAT dust guard; no combine prereq.
- ``DAEMON_AUTO``: enforce required amount; sub-CAT dust guard; combine-for-split prereq on first attempt only
  (caller passes ``allow_combine_prereq=False`` on retry).
"""

from greenfloor.core.coin_ops.gate_bridge import (
    coin_op_should_stop,
    evaluate_coin_split_gate,
)
from greenfloor.core.coin_ops._bridge import (
    coin_meets_coin_op_min_amount,
    coin_op_min_amount_mojos,
    coin_op_target_amount_allowed,
    compute_bucket_counts_from_coins,
    fee_budget_allows_execution,
    partition_plans_by_budget,
    plan_auto_combine_inputs,
    plan_auto_split_selection,
    plan_coin_ops,
    projected_coin_ops_fee_mojos,
    select_spendable_coins_for_target_amount,
    split_would_create_sub_cat_change,
)
from greenfloor.core.coin_ops.types import (
    BucketSpec,
    CoinOpPlan,
    CombineInputSelectionMode,
    SplitAutoSelectPlan,
    SplitCoinPlan,
    SplitCombinePrereqPlan,
    SplitPlanningProfile,
    SplitSkipPlan,
)

__all__ = [
    "coin_op_should_stop",
    "evaluate_coin_split_gate",
    "BucketSpec",
    "CoinOpPlan",
    "CombineInputSelectionMode",
    "SplitAutoSelectPlan",
    "SplitCoinPlan",
    "SplitCombinePrereqPlan",
    "SplitPlanningProfile",
    "SplitSkipPlan",
    "coin_meets_coin_op_min_amount",
    "coin_op_min_amount_mojos",
    "coin_op_target_amount_allowed",
    "compute_bucket_counts_from_coins",
    "fee_budget_allows_execution",
    "partition_plans_by_budget",
    "plan_auto_combine_inputs",
    "plan_auto_split_selection",
    "plan_coin_ops",
    "projected_coin_ops_fee_mojos",
    "select_spendable_coins_for_target_amount",
    "split_would_create_sub_cat_change",
]

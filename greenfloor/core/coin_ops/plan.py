from __future__ import annotations

from greenfloor.core.coin_ops._bridge import _kernel, _require_coin_op_plans
from greenfloor.core.coin_ops.types import BucketSpec, CoinOpPlan


def plan_coin_ops(
    *,
    buckets: list[BucketSpec],
    max_operations_per_run: int,
    max_fee_budget_mojos: int,
    split_fee_mojos: int,
    combine_fee_mojos: int,
) -> list[CoinOpPlan]:
    return _require_coin_op_plans(
        _kernel().plan_coin_ops(
            buckets,
            int(max_operations_per_run),
            int(max_fee_budget_mojos),
            int(split_fee_mojos),
            int(combine_fee_mojos),
        )
    )

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class BucketSpec:
    size_base_units: int
    target_count: int
    split_buffer_count: int
    combine_when_excess_factor: float
    current_count: int


@dataclass(frozen=True, slots=True)
class CoinOpPlan:
    op_type: str
    size_base_units: int
    op_count: int
    reason: str


def plan_coin_ops(
    *,
    buckets: list[BucketSpec],
    max_operations_per_run: int,
    max_fee_budget_mojos: int,
    split_fee_mojos: int,
    combine_fee_mojos: int,
) -> list[CoinOpPlan]:
    plans: list[CoinOpPlan] = []
    remaining_ops = max_operations_per_run
    remaining_fee = max_fee_budget_mojos if max_fee_budget_mojos > 0 else 10**18

    deficits: list[tuple[float, BucketSpec, int]] = []
    for b in buckets:
        threshold = b.target_count + b.split_buffer_count
        deficit = threshold - b.current_count
        if deficit > 0 and b.target_count > 0:
            deficits.append((deficit / b.target_count, b, deficit))
    deficits.sort(key=lambda x: (-x[0], x[1].size_base_units))

    for _ratio, bucket, deficit in deficits:
        if remaining_ops <= 0:
            break
        if split_fee_mojos > remaining_fee:
            break
        op_count = min(deficit, remaining_ops)
        if op_count <= 0:
            continue
        plans.append(
            CoinOpPlan(
                op_type="split",
                size_base_units=bucket.size_base_units,
                op_count=op_count,
                reason="low_watermark_buffer_deficit",
            )
        )
        remaining_ops -= op_count
        remaining_fee -= split_fee_mojos

    if deficits:
        return plans

    excess_candidates: list[tuple[BucketSpec, int]] = []
    for b in buckets:
        threshold = int(b.target_count * b.combine_when_excess_factor)
        excess = b.current_count - threshold
        if excess > 0:
            excess_candidates.append((b, excess))
    excess_candidates.sort(key=lambda x: x[0].size_base_units)

    for bucket, excess in excess_candidates:
        if remaining_ops <= 0:
            break
        if combine_fee_mojos > remaining_fee:
            break
        op_count = min(excess, remaining_ops)
        if op_count <= 0:
            continue
        plans.append(
            CoinOpPlan(
                op_type="combine",
                size_base_units=bucket.size_base_units,
                op_count=op_count,
                reason="excess_only_policy",
            )
        )
        remaining_ops -= op_count
        remaining_fee -= combine_fee_mojos

    return plans

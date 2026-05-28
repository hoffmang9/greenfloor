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

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Any


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


class CombineInputSelectionMode(StrEnum):
    LARGEST_BY_AMOUNT = "largest_by_amount"
    EXACT_AMOUNT = "exact_amount"


class SplitPlanningProfile(StrEnum):
    CLI_AUTO = "cli_auto"
    DAEMON_AUTO = "daemon_auto"


@dataclass(frozen=True, slots=True)
class SplitCombinePrereqPlan:
    input_coin_ids: list[str]
    target_amount: int
    selected_total: int
    exact_match: bool
    cap_applied: bool
    selected_count_before_cap: int
    combine_input_cap: int


@dataclass(frozen=True, slots=True)
class SplitCoinPlan:
    coin_id: str
    selected_amount_mojos: int


@dataclass(frozen=True, slots=True)
class SplitSkipPlan:
    reason: str
    data: dict[str, Any] | None = None


SplitAutoSelectPlan = SplitCoinPlan | SplitCombinePrereqPlan | SplitSkipPlan


@dataclass(frozen=True, slots=True)
class CoinSplitGateResult:
    asset_id: str
    size_base_units: int
    required_min_count: int
    current_count: int
    larger_reserve_coin_count: int
    extra_denom_coin_count: int
    reserve_ready: bool
    ready: bool

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
    """PyO3/kernel FFI shape for ``evaluate_coin_split_gate``; convert at the bridge."""

    asset_id: str
    size_base_units: int
    required_min_count: int
    current_count: int
    larger_reserve_coin_count: int
    extra_denom_coin_count: int
    reserve_ready: bool
    ready: bool


@dataclass(frozen=True, slots=True)
class CoinCombineGateResult:
    """PyO3/kernel FFI shape for ``evaluate_coin_combine_gate``; convert at the bridge."""

    asset_id: str
    size_base_units: int
    max_allowed_count: int
    current_count: int
    ready: bool


@dataclass(frozen=True, slots=True)
class SplitDenominationReadiness:
    """Split-until-ready denomination readiness."""

    asset_id: str
    size_base_units: int
    required_min_count: int
    current_count: int
    larger_reserve_coin_count: int
    extra_denom_coin_count: int
    reserve_ready: bool
    ready: bool

    @classmethod
    def from_kernel_gate(cls, gate: CoinSplitGateResult) -> SplitDenominationReadiness:
        return cls(
            asset_id=gate.asset_id,
            size_base_units=gate.size_base_units,
            required_min_count=gate.required_min_count,
            current_count=gate.current_count,
            larger_reserve_coin_count=gate.larger_reserve_coin_count,
            extra_denom_coin_count=gate.extra_denom_coin_count,
            reserve_ready=gate.reserve_ready,
            ready=gate.ready,
        )

    def to_payload(self) -> dict[str, int | bool | str]:
        return {
            "asset_id": self.asset_id,
            "size_base_units": self.size_base_units,
            "current_count": self.current_count,
            "required_min_count": self.required_min_count,
            "larger_reserve_coin_count": self.larger_reserve_coin_count,
            "extra_denom_coin_count": self.extra_denom_coin_count,
            "reserve_ready": self.reserve_ready,
            "ready": self.ready,
        }


@dataclass(frozen=True, slots=True)
class CombineDenominationReadiness:
    """Combine-until-ready denomination readiness (excess denomination coin cap)."""

    asset_id: str
    size_base_units: int
    max_allowed_count: int
    current_count: int
    ready: bool

    @classmethod
    def from_kernel_gate(cls, gate: CoinCombineGateResult) -> CombineDenominationReadiness:
        return cls(
            asset_id=gate.asset_id,
            size_base_units=gate.size_base_units,
            max_allowed_count=gate.max_allowed_count,
            current_count=gate.current_count,
            ready=gate.ready,
        )

    def to_payload(self) -> dict[str, int | bool | str]:
        return {
            "asset_id": self.asset_id,
            "size_base_units": self.size_base_units,
            "current_count": self.current_count,
            "max_allowed_count": self.max_allowed_count,
            "ready": self.ready,
        }


DenominationReadiness = SplitDenominationReadiness | CombineDenominationReadiness


def readiness_to_payload(readiness: DenominationReadiness) -> dict[str, int | bool | str]:
    return readiness.to_payload()

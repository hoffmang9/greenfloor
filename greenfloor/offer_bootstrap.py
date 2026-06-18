from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Literal

__all__ = [
    "BootstrapCoin",
    "BootstrapPhaseResult",
    "BootstrapPlan",
    "BootstrapPlanOutcome",
    "BootstrapPlanKind",
    "LadderDeficit",
    "PlannerLadderRow",
]

BootstrapPlanKind = Literal[
    "ready",
    "needs_split",
    "cannot_fund",
    "invalid_ladder",
    "invalid_coins",
]

BootstrapPhaseStatus = Literal["skipped", "failed", "executed"]


@dataclass(frozen=True, slots=True)
class BootstrapPhaseResult:
    """Typed bootstrap phase DTO for PyO3 bridge boundaries."""

    status: BootstrapPhaseStatus
    reason: str
    ready: bool = False
    fee_mojos: int = 0
    fee_source: str = ""
    fee_lookup_error: str | None = None
    wait_error: str | None = None
    split_result: dict[str, Any] = field(default_factory=dict)
    wait_events: list[dict[str, str]] = field(default_factory=list)
    plan: BootstrapPlan | None = None


@dataclass(frozen=True, slots=True)
class LadderDeficit:
    size_base_units: int
    required_count: int
    current_count: int
    deficit_count: int


@dataclass(frozen=True, slots=True)
class PlannerLadderRow:
    size_base_units: int
    target_count: int
    split_buffer_count: int


@dataclass(frozen=True, slots=True)
class BootstrapCoin:
    id: str
    amount: int


@dataclass(frozen=True, slots=True)
class BootstrapPlan:
    source_coin_id: str
    source_amount: int
    output_amounts_base_units: list[int]
    total_output_amount: int
    change_amount: int
    deficits: list[LadderDeficit]


@dataclass(frozen=True, slots=True)
class BootstrapPlanOutcome:
    kind: BootstrapPlanKind
    plan: BootstrapPlan | None = None
    total_output_amount: int | None = None

    def __post_init__(self) -> None:
        if self.kind == "needs_split":
            if self.plan is None:
                raise ValueError("BootstrapPlanOutcome needs_split requires plan")
            if self.total_output_amount is not None:
                raise ValueError(
                    "BootstrapPlanOutcome needs_split must not set total_output_amount"
                )
            return
        if self.kind == "cannot_fund":
            if self.plan is not None:
                raise ValueError("BootstrapPlanOutcome cannot_fund must not set plan")
            if self.total_output_amount is None:
                raise ValueError("BootstrapPlanOutcome cannot_fund requires total_output_amount")
            return
        if self.plan is not None:
            raise ValueError(f"BootstrapPlanOutcome {self.kind} must not set plan")
        if self.total_output_amount is not None:
            raise ValueError(f"BootstrapPlanOutcome {self.kind} must not set total_output_amount")

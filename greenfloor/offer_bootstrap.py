from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

__all__ = [
    "BootstrapCoin",
    "BootstrapPlan",
    "BootstrapPlanOutcome",
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

    @classmethod
    def ready(cls) -> BootstrapPlanOutcome:
        return cls(kind="ready")

    @classmethod
    def needs_split(cls, plan: BootstrapPlan) -> BootstrapPlanOutcome:
        return cls(kind="needs_split", plan=plan)

    @classmethod
    def cannot_fund(cls, *, total_output_amount: int) -> BootstrapPlanOutcome:
        return cls(kind="cannot_fund", total_output_amount=total_output_amount)

    @classmethod
    def invalid_ladder(cls) -> BootstrapPlanOutcome:
        return cls(kind="invalid_ladder")

    @classmethod
    def invalid_coins(cls) -> BootstrapPlanOutcome:
        return cls(kind="invalid_coins")

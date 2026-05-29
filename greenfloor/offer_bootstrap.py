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
    """Typed bootstrap phase output for offer orchestration."""

    status: BootstrapPhaseStatus
    reason: str
    ready: bool = False
    fee_mojos: int = 0
    fee_source: str = ""
    fee_lookup_error: str | None = None
    wait_error: str | None = None
    split_result: dict[str, Any] = field(default_factory=dict)
    wait_events: list[dict[str, str]] = field(default_factory=list)
    plan: dict[str, Any] | None = None

    def to_manager_dict(self) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "status": self.status,
            "reason": self.reason,
            "ready": self.ready,
            "fee_mojos": self.fee_mojos,
            "fee_source": self.fee_source,
            "fee_lookup_error": self.fee_lookup_error,
        }
        if self.wait_error is not None:
            payload["wait_error"] = self.wait_error
        if self.split_result:
            payload["split_result"] = dict(self.split_result)
        if self.wait_events:
            payload["wait_events"] = list(self.wait_events)
        if self.plan is not None:
            payload["plan"] = dict(self.plan)
        return payload


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

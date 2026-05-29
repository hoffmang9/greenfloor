from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Literal

__all__ = [
    "BootstrapCoin",
    "BootstrapPhaseResult",
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

    def to_early_phase_result(self) -> BootstrapPhaseResult | None:
        """Map to a phase result when offer creation should not proceed to mixed-split."""
        if self.kind == "ready":
            return BootstrapPhaseResult(status="skipped", reason="already_ready")
        if self.kind == "cannot_fund":
            total = int(self.total_output_amount or 0)
            return BootstrapPhaseResult(
                status="skipped",
                reason=f"bootstrap_underfunded:total_output_amount={total}",
            )
        if self.kind == "invalid_ladder":
            return BootstrapPhaseResult(
                status="failed",
                reason="bootstrap_failed:bootstrap_invalid_ladder",
            )
        if self.kind == "invalid_coins":
            return BootstrapPhaseResult(
                status="failed",
                reason="bootstrap_failed:bootstrap_invalid_coins",
            )
        if self.kind == "needs_split":
            return None
        raise ValueError(f"unsupported_bootstrap_plan_outcome:{self.kind}")

    def require_plan(self) -> BootstrapPlan:
        if self.kind != "needs_split" or self.plan is None:
            raise ValueError("bootstrap planner outcome is not needs_split")
        return self.plan

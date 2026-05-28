"""Offer-size-gap reseed planning (Rust FFI)."""

from __future__ import annotations

from dataclasses import dataclass

from greenfloor.core.cycle._bridge import plan_reseed_actions_from_gap as plan_reseed_actions_from_gap_rust
from greenfloor.core.planned_action import PlannedAction, planned_actions_from_signer_list


@dataclass(frozen=True, slots=True)
class ReseedGapPlan:
    actions: list[PlannedAction]
    skip_reason: str | None


def plan_reseed_actions_from_gap(
    *,
    strategy_actions: list[PlannedAction],
    active_counts_by_size: dict[int, int],
    target_counts_by_size: dict[int, int],
    seed_candidates: list[PlannedAction],
) -> ReseedGapPlan:
    raw = plan_reseed_actions_from_gap_rust(
        strategy_actions=strategy_actions,
        active_counts_by_size=active_counts_by_size,
        target_counts_by_size=target_counts_by_size,
        seed_candidates=seed_candidates,
    )
    skip = raw.get("skip_reason")
    return ReseedGapPlan(
        actions=planned_actions_from_signer_list(raw["actions"]),
        skip_reason=None if skip is None else str(skip),
    )

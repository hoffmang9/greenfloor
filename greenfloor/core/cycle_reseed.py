"""Offer-size-gap reseed planning (Rust FFI types)."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

from greenfloor.core.planned_action import PlannedAction


class ReseedSkipReason(StrEnum):
    """Member values must match `reseed_skip_reason_labels()` from the Rust reseed engine."""

    STRATEGY_ACTIONS_PRESENT = "strategy_actions_present"
    ACTIVE_OFFER_TARGETS_SATISFIED = "active_offer_targets_satisfied"
    NO_SEED_CANDIDATES = "no_seed_candidates"
    MISSING_SIZES_NO_SEED_TEMPLATE = "missing_sizes_no_seed_template"
    RESEED_ZERO_REPEAT_FILTERED = "reseed_zero_repeat_filtered"


@dataclass(frozen=True, slots=True)
class ReseedGapPlan:
    actions: list[PlannedAction]
    skip_reason: ReseedSkipReason | None
    missing_by_size: dict[int, int]


def python_reseed_skip_reason_labels() -> frozenset[str]:
    return frozenset(member.value for member in ReseedSkipReason)

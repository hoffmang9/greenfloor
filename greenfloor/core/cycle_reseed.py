"""Offer-size-gap reseed planning (Rust FFI types)."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from greenfloor.core.planned_action import PlannedAction, planned_actions_from_signer_list


class ReseedSkipReason(StrEnum):
    STRATEGY_ACTIONS_PRESENT = "strategy_actions_present"
    ACTIVE_OFFER_TARGETS_SATISFIED = "active_offer_targets_satisfied"
    NO_SEED_CANDIDATES = "no_seed_candidates"
    MISSING_SIZES_NO_SEED_TEMPLATE = "missing_sizes_no_seed_template"
    RESEED_ZERO_REPEAT_FILTERED = "reseed_zero_repeat_filtered"

    @classmethod
    def from_label(cls, label: str | None) -> ReseedSkipReason | None:
        if label is None:
            return None
        return cls(str(label))


@dataclass(frozen=True, slots=True)
class ReseedGapPlan:
    actions: list[PlannedAction]
    skip_reason: ReseedSkipReason | None
    missing_by_size: dict[int, int]


def _normalize_skip_reason(skip: ReseedSkipReason | str | None) -> ReseedSkipReason | None:
    if skip is None:
        return None
    if isinstance(skip, ReseedSkipReason):
        return skip
    return ReseedSkipReason.from_label(str(skip))


def reseed_gap_plan_from_signer(raw: Any) -> ReseedGapPlan:
    if isinstance(raw, ReseedGapPlan):
        return ReseedGapPlan(
            actions=planned_actions_from_signer_list(raw.actions),
            skip_reason=_normalize_skip_reason(raw.skip_reason),
            missing_by_size={int(size): int(count) for size, count in raw.missing_by_size.items()},
        )
    actions = raw["actions"]
    skip_raw = raw["skip_reason"]
    missing_raw = raw["missing_by_size"]
    return ReseedGapPlan(
        actions=planned_actions_from_signer_list(actions),
        skip_reason=_normalize_skip_reason(
            None if skip_raw is None else str(skip_raw)
        ),
        missing_by_size={int(size): int(count) for size, count in missing_raw.items()},
    )

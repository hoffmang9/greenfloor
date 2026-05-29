"""Planned offer action model and signer FFI conversion (no cycle bridge imports)."""

from __future__ import annotations

from dataclasses import dataclass

# Cycle kernel and strategy evaluation emit only these labels.
_PLANNED_OFFER_SIDES = frozenset({"buy", "sell"})


@dataclass(frozen=True, slots=True)
class PlannedAction:
    size: int
    repeat: int
    pair: str
    expiry_unit: str
    expiry_value: int
    cancel_after_create: bool
    reason: str
    target_spread_bps: int | None = None
    side: str = "sell"


def planned_action_side(action: PlannedAction) -> str:
    """Return ``buy`` or ``sell`` without a kernel round-trip when side is already canonical."""
    side = str(action.side or "sell").strip().lower()
    if side in _PLANNED_OFFER_SIDES:
        return side
    from greenfloor.core.offer_policy import normalize_offer_side

    return normalize_offer_side(side)


def planned_actions_from_signer_list(result: object) -> list[PlannedAction]:
    """Accept PyO3 ``PlannedAction`` lists from the signer extension."""
    if not isinstance(result, list):
        raise TypeError("signer planned-action call returned non-list result")
    for index, item in enumerate(result):
        if not isinstance(item, PlannedAction):
            raise TypeError(
                f"signer planned-action list item {index} must be PlannedAction, "
                f"got {type(item).__name__}"
            )
    return result

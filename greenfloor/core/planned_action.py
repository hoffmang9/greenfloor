"""Planned offer action model and signer FFI conversion (no cycle bridge imports)."""

from __future__ import annotations

from dataclasses import dataclass


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

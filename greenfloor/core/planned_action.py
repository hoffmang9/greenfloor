"""Planned offer action model and signer FFI conversion (no cycle bridge imports)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


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


def _signer_item_value(item: Any, key: str, default: Any = None) -> Any:
    if isinstance(item, PlannedAction):
        return getattr(item, key)
    if hasattr(item, key):
        return getattr(item, key)
    if hasattr(item, "get"):
        return item.get(key, default)
    return item[key]


def planned_action_from_signer_item(item: Any) -> PlannedAction:
    """Normalize a legacy dict-like signer row to PlannedAction."""
    if isinstance(item, PlannedAction):
        return item
    target_spread_bps = _signer_item_value(item, "target_spread_bps")
    return PlannedAction(
        size=int(_signer_item_value(item, "size")),
        repeat=int(_signer_item_value(item, "repeat")),
        pair=str(_signer_item_value(item, "pair")),
        expiry_unit=str(_signer_item_value(item, "expiry_unit")),
        expiry_value=int(_signer_item_value(item, "expiry_value")),
        cancel_after_create=bool(_signer_item_value(item, "cancel_after_create")),
        reason=str(_signer_item_value(item, "reason")),
        target_spread_bps=(
            int(target_spread_bps) if target_spread_bps is not None else None
        ),
        side=str(_signer_item_value(item, "side", "sell")),
    )


def planned_actions_from_signer_list(result: Any) -> list[PlannedAction]:
    """Accept PyO3 ``PlannedAction`` lists; coerce legacy dict rows only when needed."""
    if not isinstance(result, list):
        raise TypeError("signer planned-action call returned non-list result")
    if not result:
        return []
    if all(isinstance(item, PlannedAction) for item in result):
        return result
    return [planned_action_from_signer_item(item) for item in result]


planned_action_from_rust_dict = planned_action_from_signer_item

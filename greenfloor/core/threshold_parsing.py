"""Cancel-move threshold resolution at config and runtime boundaries."""

from __future__ import annotations

import os
from typing import Any


def parse_optional_positive_int(value: object) -> int | None:
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, int):
        parsed = value
    elif isinstance(value, str):
        stripped = value.strip()
        if not stripped:
            return None
        try:
            parsed = int(stripped)
        except ValueError:
            return None
    else:
        return None
    if parsed <= 0:
        return None
    return parsed


def unstable_cancel_move_threshold_bps_from_env() -> int | None:
    return parse_optional_positive_int(os.getenv("GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS", "").strip())


def resolved_market_cancel_move_threshold_bps(market: Any) -> int | None:
    typed = getattr(market, "cancel_move_threshold_bps", None)
    if typed is not None:
        return int(typed)
    pricing = dict(getattr(market, "pricing", {}) or {})
    return parse_optional_positive_int(pricing.get("cancel_move_threshold_bps"))

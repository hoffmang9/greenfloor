"""Shared positive-integer threshold parsing at the Python config boundary."""

from __future__ import annotations


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


def parse_cancel_move_thresholds(
    *,
    market_threshold_raw: object = None,
    env_raw: str = "",
) -> tuple[int | None, int | None]:
    market_threshold = parse_optional_positive_int(market_threshold_raw)
    env_threshold = (
        parse_optional_positive_int(env_raw.strip()) if env_raw.strip() else None
    )
    return market_threshold, env_threshold

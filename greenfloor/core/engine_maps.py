"""Shared coercion helpers for engine dict return values."""

from __future__ import annotations


def require_i64_i64_map(value: object, *, label: str = "engine") -> dict[int, int]:
    if not isinstance(value, dict):
        raise TypeError(f"{label} returned non-dict result")
    return {int(key): int(val) for key, val in value.items()}


def require_side_offer_count_maps(
    value: object,
    *,
    label: str,
) -> dict[str, dict[int, int]]:
    if not isinstance(value, dict):
        raise TypeError(f"{label} returned non-dict result")
    return {
        "buy": require_i64_i64_map(value.get("buy", {}), label=f"{label}.buy"),
        "sell": require_i64_i64_map(value.get("sell", {}), label=f"{label}.sell"),
    }

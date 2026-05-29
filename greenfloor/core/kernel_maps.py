"""Shared coercion helpers for kernel dict return values."""

from __future__ import annotations


def require_i64_i64_map(value: object, *, label: str = "kernel") -> dict[int, int]:
    if not isinstance(value, dict):
        raise TypeError(f"{label} returned non-dict result")
    return {int(key): int(val) for key, val in value.items()}

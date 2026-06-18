"""Shared hex identifier utilities for vault scan scripts.

Canonical validation lives in ``greenfloor-engine`` (`hex.rs`); this module mirrors
the script-facing surface for synchronous in-process use.
"""

from __future__ import annotations

_CANONICAL_XCH_MOJOS = 1_000_000_000_000
_CANONICAL_CAT_MOJOS = 1_000
_XCH_SYMBOLS = frozenset({"xch", "txch", "1"})


def normalize_hex_id(value: object) -> str:
    if not isinstance(value, str):
        return ""
    normalized = value.strip().lower()
    if normalized.startswith("0x"):
        normalized = normalized[2:]
    if len(normalized) != 64:
        return ""
    if not all(ch in "0123456789abcdef" for ch in normalized):
        return ""
    return normalized


def is_hex_id(value: str) -> bool:
    return bool(normalize_hex_id(str(value)))


def canonical_is_xch(asset_id: str) -> bool:
    return str(asset_id or "").strip().lower() in _XCH_SYMBOLS


def default_mojo_multiplier_for_asset(asset_id: str) -> int:
    return _CANONICAL_XCH_MOJOS if canonical_is_xch(asset_id) else _CANONICAL_CAT_MOJOS

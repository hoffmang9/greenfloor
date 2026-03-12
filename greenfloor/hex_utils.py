"""Shared hex identifier utilities.

Canonical validation and normalization for 64-character hex identifiers
(asset IDs, coin IDs, transaction IDs), plus shared asset-type helpers.
"""

from __future__ import annotations

_HEX_CHARS = frozenset("0123456789abcdef")
# Symbols that identify the native XCH/TXCH coin (asset id "1" is the Chia
# internal representation used in some wallet APIs).
_XCH_ASSET_SYMBOLS = frozenset({"xch", "txch", "1"})
_CANONICAL_XCH_MOJOS = 1_000_000_000_000
_CANONICAL_CAT_MOJOS = 1_000


def is_hex_id(value: str) -> bool:
    """Return True if *value* is a 64-character lowercase hex string (optionally 0x-prefixed)."""
    normalized = value.strip().lower()
    if normalized.startswith("0x"):
        normalized = normalized[2:]
    return len(normalized) == 64 and all(ch in _HEX_CHARS for ch in normalized)


def canonical_is_xch(asset_id: str) -> bool:
    """Return True if *asset_id* refers to the native XCH/TXCH coin."""
    return str(asset_id or "").strip().lower() in _XCH_ASSET_SYMBOLS


def default_mojo_multiplier_for_asset(asset_id: str) -> int:
    """Return the canonical mojo-per-unit multiplier for *asset_id*.

    XCH/TXCH uses 10^12 mojos per XCH; CATs use 1 000 mojos per CAT unit.
    """
    return _CANONICAL_XCH_MOJOS if canonical_is_xch(asset_id) else _CANONICAL_CAT_MOJOS


def normalize_hex_id(value: object) -> str:
    """Normalize a hex identifier: strip, lowercase, remove 0x prefix.

    Returns the 64-char hex string, or empty string if invalid.
    """
    if not isinstance(value, str):
        return ""
    normalized = value.strip().lower()
    if normalized.startswith("0x"):
        normalized = normalized[2:]
    if len(normalized) != 64:
        return ""
    if not all(ch in _HEX_CHARS for ch in normalized):
        return ""
    return normalized

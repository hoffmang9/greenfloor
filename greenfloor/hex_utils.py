"""Shared hex identifier utilities.

Canonical validation and normalization for 64-character hex identifiers
(asset IDs, coin IDs, transaction IDs), plus shared asset-type helpers.
"""

from __future__ import annotations

_CANONICAL_XCH_MOJOS = 1_000_000_000_000
_CANONICAL_CAT_MOJOS = 1_000
_XCH_SYMBOLS = frozenset({"xch", "txch", "1"})


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
    if not all(ch in "0123456789abcdef" for ch in normalized):
        return ""
    return normalized


def is_hex_id(value: str) -> bool:
    """Return True if *value* is a 64-character lowercase hex string (optionally 0x-prefixed)."""
    return bool(normalize_hex_id(str(value)))


def canonical_is_xch(asset_id: str) -> bool:
    """Return True when *asset_id* is native XCH/TXCH (``xch``, ``txch``, or ``1``).

    Empty or whitespace returns False. Rust ``is_xch_like_asset`` also treats empty as
    XCH-like for signer payloads; use that only at the FFI boundary, not here.
    """
    return str(asset_id or "").strip().lower() in _XCH_SYMBOLS


def default_mojo_multiplier_for_asset(asset_id: str) -> int:
    """Return the canonical mojo-per-unit multiplier for *asset_id*.

    XCH/TXCH uses 10^12 mojos per XCH; CATs use 1 000 mojos per CAT unit.
    """
    return _CANONICAL_XCH_MOJOS if canonical_is_xch(asset_id) else _CANONICAL_CAT_MOJOS

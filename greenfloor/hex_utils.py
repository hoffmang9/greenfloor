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
    """Return True when *asset_id* is an explicit native XCH/TXCH symbol.

    Use for policy, catalog, coin listing, and Python gates that must not treat a
    missing asset id as XCH. Empty string returns False.
    """
    return str(asset_id or "").strip().lower() in _XCH_ASSET_SYMBOLS


def is_xch_like_asset_id(asset_id: str) -> bool:
    """Return True when *asset_id* should follow the Rust signer XCH code path.

    Matches ``greenfloor_signer::coinset::is_xch_like_asset``: empty/whitespace is
    treated as XCH-like so payloads omitted by callers still reach the native path.
    Do not use for Python-only scope gates (use :func:`canonical_is_xch` instead).
    """
    normalized = str(asset_id or "").strip().lower()
    return not normalized or normalized in _XCH_ASSET_SYMBOLS


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

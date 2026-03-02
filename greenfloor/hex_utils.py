"""Shared hex identifier utilities.

Canonical validation and normalization for 64-character hex identifiers
(asset IDs, coin IDs, transaction IDs).
"""

from __future__ import annotations

_HEX_CHARS = frozenset("0123456789abcdef")


def is_hex_id(value: str) -> bool:
    """Return True if *value* is a 64-character lowercase hex string (optionally 0x-prefixed)."""
    normalized = value.strip().lower()
    if normalized.startswith("0x"):
        normalized = normalized[2:]
    return len(normalized) == 64 and all(ch in _HEX_CHARS for ch in normalized)


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

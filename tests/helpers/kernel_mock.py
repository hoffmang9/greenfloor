"""Minimal greenfloor_signer stub helpers for partial kernel mocks in tests."""

from __future__ import annotations


def mock_kernel_normalize_hex_id(value: str) -> str:
    """Mirror ``hex_utils.normalize_hex_id`` for tests that stub ``greenfloor_signer``."""
    normalized = value.strip().lower()
    if normalized.startswith("0x"):
        normalized = normalized[2:]
    if len(normalized) != 64:
        return ""
    if not all(ch in "0123456789abcdef" for ch in normalized):
        return ""
    return normalized

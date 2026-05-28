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


def mock_kernel_is_hex_id(value: str) -> bool:
    return bool(mock_kernel_normalize_hex_id(value))


def mock_kernel_canonical_is_xch(asset_id: str) -> bool:
    lowered = str(asset_id or "").strip().lower()
    return lowered in {"xch", "txch", "1"}


def mock_kernel_default_mojo_multiplier_for_asset(asset_id: str) -> int:
    return 1_000_000_000_000 if mock_kernel_canonical_is_xch(asset_id) else 1_000


class MinimalSignerKernel:
    """Base stub for tests that patch ``sys.modules['greenfloor_signer']``.

    Subclass and override only the symbols your test exercises. Hex helpers and
    ``validate_offer`` are provided by default so offer verification tests do not
    need to enumerate every kernel export.
    """

    @staticmethod
    def validate_offer(_offer: str) -> None:
        return None

    normalize_hex_id = staticmethod(mock_kernel_normalize_hex_id)
    is_hex_id = staticmethod(mock_kernel_is_hex_id)
    canonical_is_xch = staticmethod(mock_kernel_canonical_is_xch)
    default_mojo_multiplier_for_asset = staticmethod(mock_kernel_default_mojo_multiplier_for_asset)

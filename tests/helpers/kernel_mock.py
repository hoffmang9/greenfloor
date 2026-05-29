"""Minimal greenfloor kernel stub helpers for partial kernel mocks in tests."""

from __future__ import annotations

from typing import Any


def install_kernel_stub(monkeypatch: Any, stub: Any) -> None:
    """Register a stub for both ADR 0010 module names."""
    monkeypatch.setitem(__import__("sys").modules, "greenfloor_kernel", stub)
    monkeypatch.setitem(__import__("sys").modules, "greenfloor_signer", stub)


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


def mock_kernel_bootstrap_block_error(
    bootstrap_status: str,
    bootstrap_reason: str,
    bootstrap_ready: bool,
) -> str | None:
    status = str(bootstrap_status).strip().lower()
    reason = str(bootstrap_reason).strip() or "bootstrap_precheck_failed"
    if status == "failed":
        return f"bootstrap_failed:{reason}"
    if status == "executed" and not bool(bootstrap_ready):
        return f"bootstrap_pending:{reason}"
    if status == "skipped" and reason != "already_ready":
        return f"bootstrap_precheck_skipped:{reason}"
    return None


def mock_kernel_dexie_offer_asset_expectation_error(
    offered: object,
    requested: object,
    expected_offered_asset_id: str,
    expected_offered_symbol: str,
    expected_requested_asset_id: str,
    expected_requested_symbol: str,
) -> str | None:
    def _matches_row(row: object, *, expected_asset: str, expected_symbol: str) -> bool:
        if not isinstance(row, dict):
            return False
        row_id = str(row.get("id", "")).strip().lower()
        if row_id == expected_asset:
            return True
        if not expected_symbol:
            return False
        return (
            str(row.get("code", "")).strip().lower() == expected_symbol
            or str(row.get("name", "")).strip().lower() == expected_symbol
        )

    expected_offered_asset = str(expected_offered_asset_id).strip().lower()
    expected_offered = str(expected_offered_symbol).strip().lower()
    if expected_offered_asset and isinstance(offered, list):
        if not any(
            _matches_row(
                row,
                expected_asset=expected_offered_asset,
                expected_symbol=expected_offered,
            )
            for row in offered
        ):
            return (
                "dexie_offer_offered_asset_missing:"
                f"expected_asset={expected_offered_asset_id}:"
                f"expected_symbol={expected_offered_symbol}"
            )

    expected_requested_asset = str(expected_requested_asset_id).strip().lower()
    expected_requested = str(expected_requested_symbol).strip().lower()
    if expected_requested_asset and isinstance(requested, list):
        if not any(
            _matches_row(
                row,
                expected_asset=expected_requested_asset,
                expected_symbol=expected_requested,
            )
            for row in requested
        ):
            return (
                "dexie_offer_requested_asset_missing:"
                f"expected_asset={expected_requested_asset_id}:"
                f"expected_symbol={expected_requested_symbol}"
            )
    return None


def mock_kernel_expected_publish_asset_fields(
    side: str,
    base_symbol: str,
    quote_asset: str,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
) -> dict[str, str]:
    is_buy = str(side).strip().lower() == "buy"
    if is_buy:
        return {
            "expected_offered_asset_id": str(resolved_quote_asset_id),
            "expected_offered_symbol": str(quote_asset),
            "expected_requested_asset_id": str(resolved_base_asset_id),
            "expected_requested_symbol": str(base_symbol),
        }
    return {
        "expected_offered_asset_id": str(resolved_base_asset_id),
        "expected_offered_symbol": str(base_symbol),
        "expected_requested_asset_id": str(resolved_quote_asset_id),
        "expected_requested_symbol": str(quote_asset),
    }


class MinimalSignerKernel:
    """Base stub for tests that patch ``sys.modules['greenfloor_signer']``.

    Subclass and override only the symbols your test exercises. Hex helpers,
    offer-build pricing helpers, and Dexie verification are provided by default
    so CLI/offer tests do not need to enumerate every kernel export.
    """

    @staticmethod
    def validate_offer(_offer: str) -> None:
        return None

    @staticmethod
    def verify_offer_for_dexie(_offer: str) -> None:
        return None

    @staticmethod
    def mojo_multiplier_for_leg(pricing: object, field: str, asset_id: str) -> int:
        pricing_dict = pricing if isinstance(pricing, dict) else {}
        if field in pricing_dict:
            return int(pricing_dict[field])
        return mock_kernel_default_mojo_multiplier_for_asset(asset_id)

    @staticmethod
    def resolve_offer_expiry_for_pricing(pricing: object) -> tuple[str, int]:
        pricing_dict = pricing if isinstance(pricing, dict) else {}
        return ("minutes", int(pricing_dict.get("strategy_offer_expiry_minutes", 60)))

    @staticmethod
    def resolve_quote_price_for_pricing(pricing: object) -> float:
        pricing_dict = pricing if isinstance(pricing, dict) else {}
        return float(pricing_dict.get("fixed_quote_per_base", 1.0))

    bootstrap_block_error = staticmethod(mock_kernel_bootstrap_block_error)
    expected_publish_asset_fields = staticmethod(mock_kernel_expected_publish_asset_fields)
    dexie_offer_asset_expectation_error = staticmethod(
        mock_kernel_dexie_offer_asset_expectation_error
    )

    normalize_hex_id = staticmethod(mock_kernel_normalize_hex_id)
    is_hex_id = staticmethod(mock_kernel_is_hex_id)
    canonical_is_xch = staticmethod(mock_kernel_canonical_is_xch)
    default_mojo_multiplier_for_asset = staticmethod(mock_kernel_default_mojo_multiplier_for_asset)

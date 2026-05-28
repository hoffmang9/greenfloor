"""Rust-backed offer build shaping and Dexie validation."""

from __future__ import annotations

from typing import Any

from greenfloor.core.kernel_bridge import policy_kernel


def resolve_offer_expiry_for_pricing(pricing: dict[str, Any]) -> tuple[str, int]:
    unit, value = policy_kernel().resolve_offer_expiry_for_pricing(pricing)
    return str(unit), int(value)


def resolve_quote_price_for_pricing(pricing: dict[str, Any]) -> float:
    return float(policy_kernel().resolve_quote_price_for_pricing(pricing))


def mojo_multiplier_for_leg(pricing: dict[str, Any], field: str, asset_id: str) -> int:
    return int(policy_kernel().mojo_multiplier_for_leg(pricing, field, asset_id))


def verify_offer_for_dexie(offer_text: str) -> str | None:
    try:
        error = policy_kernel().verify_offer_for_dexie(offer_text)
    except ImportError:
        return "wallet_sdk_import_error:greenfloor_signer_unavailable"
    return None if error is None else str(error)

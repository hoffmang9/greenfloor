"""Rust-backed offer build pricing and expiry shaping."""

from __future__ import annotations

from typing import Any

from greenfloor.core.kernel_bridge import import_kernel


def resolve_offer_expiry_for_pricing(pricing: dict[str, Any]) -> tuple[str, int]:
    unit, value = import_kernel().resolve_offer_expiry_for_pricing(pricing)
    return str(unit), int(value)


def resolve_quote_price_for_pricing(pricing: dict[str, Any]) -> float:
    return float(import_kernel().resolve_quote_price_for_pricing(pricing))


def mojo_multiplier_for_leg(pricing: dict[str, Any], field: str, asset_id: str) -> int:
    return int(import_kernel().mojo_multiplier_for_leg(pricing, field, asset_id))

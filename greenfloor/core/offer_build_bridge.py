"""Rust-backed offer build context helpers (expiry, quote price, mojo multipliers)."""

from __future__ import annotations

from typing import Any

from greenfloor.core import engine_bridge

__all__ = [
    "mojo_multiplier_for_leg",
    "resolve_offer_expiry_for_pricing",
    "resolve_quote_price_for_pricing",
]


def resolve_offer_expiry_for_pricing(pricing: dict[str, Any]) -> tuple[str, int]:
    unit, value = engine_bridge.policy_engine().resolve_offer_expiry_for_pricing(pricing)
    return str(unit), int(value)


def resolve_quote_price_for_pricing(pricing: dict[str, Any]) -> float:
    return float(engine_bridge.policy_engine().resolve_quote_price_for_pricing(pricing))


def mojo_multiplier_for_leg(pricing: dict[str, Any], field: str, asset_id: str) -> int:
    return int(engine_bridge.policy_engine().mojo_multiplier_for_leg(pricing, field, asset_id))

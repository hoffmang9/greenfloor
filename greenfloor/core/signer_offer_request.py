"""Deterministic signer ``create_offer`` request dict construction (no IO)."""

from __future__ import annotations

from typing import Any

from greenfloor.config.models import MarketConfig
from greenfloor.core.policy_bridge import mojo_multiplier_for_leg


def normalize_action_side(value: str | None) -> str:
    side = str(value or "").strip().lower()
    return "buy" if side == "buy" else "sell"


def build_signer_create_offer_request(
    *,
    market: MarketConfig,
    size_base_units: int,
    quote_price: float,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    action_side: str = "sell",
    split_input_coins: bool = True,
    broadcast_split: bool = True,
    expires_at_unix: int | None = None,
) -> dict[str, Any]:
    """Build the dict passed to ``rust_signer.build_vault_cat_offer``."""
    side = normalize_action_side(action_side)
    pricing = dict(market.pricing or {})
    base_mult = int(
        mojo_multiplier_for_leg(
            pricing,
            "base_unit_mojo_multiplier",
            str(resolved_base_asset_id),
        )
    )
    quote_mult = int(
        mojo_multiplier_for_leg(
            pricing,
            "quote_unit_mojo_multiplier",
            str(resolved_quote_asset_id),
        )
    )
    offer_amount = int(size_base_units) * base_mult
    request_amount = int(round(float(size_base_units) * float(quote_price) * float(quote_mult)))
    if request_amount <= 0:
        raise ValueError("request_amount must be positive")

    if side == "buy":
        offer_asset_id = str(resolved_quote_asset_id).strip()
        request_asset_id = str(resolved_base_asset_id).strip()
        offer_amount_mojos = request_amount
        request_amount_mojos = offer_amount
    else:
        offer_asset_id = str(resolved_base_asset_id).strip()
        request_asset_id = str(resolved_quote_asset_id).strip()
        offer_amount_mojos = offer_amount
        request_amount_mojos = request_amount

    receive_address = str(market.receive_address or "").strip()
    if not receive_address:
        raise ValueError("market.receive_address is required for signer offer build")

    return {
        "receive_address": receive_address,
        "offer_asset_id": offer_asset_id.removeprefix("0x").lower(),
        "offer_amount": int(offer_amount_mojos),
        "request_asset_id": request_asset_id.removeprefix("0x").lower(),
        "request_amount": int(request_amount_mojos),
        "offer_coin_ids": [],
        "presplit_coin_ids": [],
        "split_input_coins": bool(split_input_coins),
        "broadcast_split": bool(broadcast_split),
        "expires_at": expires_at_unix,
    }

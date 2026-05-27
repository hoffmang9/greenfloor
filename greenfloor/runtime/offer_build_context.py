"""Shared context for manual offer build-and-post CLI paths."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from greenfloor.config.io import resolve_quote_asset_for_offer
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.hex_utils import default_mojo_multiplier_for_asset
from greenfloor.runtime.offer_publish import (
    normalize_offer_side,
    resolve_offer_expiry_for_market,
    resolve_quote_price_for_market,
)


@dataclass(frozen=True, slots=True)
class OfferBuildContext:
    program: ProgramConfig
    market: MarketConfig
    program_path: Path
    network: str
    keyring_yaml_path: str
    resolved_quote_asset: str
    expiry_unit: str
    expiry_value: int
    base_unit_mojo_multiplier: int
    quote_unit_mojo_multiplier: int
    quote_price: float
    action_side: str


def prepare_offer_build_context(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    program_path: Path,
    network: str,
    keyring_yaml_path: str,
    action_side: str = "sell",
) -> OfferBuildContext:
    pricing = dict(market.pricing or {})
    resolved_quote_asset = resolve_quote_asset_for_offer(
        quote_asset=str(market.quote_asset),
        network=network,
    )
    base_unit_mojo_multiplier = int(
        pricing.get(
            "base_unit_mojo_multiplier",
            default_mojo_multiplier_for_asset(str(market.base_asset)),
        )
    )
    quote_unit_mojo_multiplier = int(
        pricing.get(
            "quote_unit_mojo_multiplier",
            default_mojo_multiplier_for_asset(str(resolved_quote_asset)),
        )
    )
    expiry_unit, expiry_value = resolve_offer_expiry_for_market(market)
    quote_price = resolve_quote_price_for_market(market)
    return OfferBuildContext(
        program=program,
        market=market,
        program_path=program_path,
        network=network,
        keyring_yaml_path=keyring_yaml_path,
        resolved_quote_asset=resolved_quote_asset,
        expiry_unit=expiry_unit,
        expiry_value=int(expiry_value),
        base_unit_mojo_multiplier=base_unit_mojo_multiplier,
        quote_unit_mojo_multiplier=quote_unit_mojo_multiplier,
        quote_price=float(quote_price),
        action_side=normalize_offer_side(action_side),
    )

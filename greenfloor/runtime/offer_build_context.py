"""Shared context for manual offer build-and-post CLI paths."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from greenfloor.config.io import resolve_quote_asset_for_offer
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.offer_request_bridge import (
    mojo_multiplier_for_leg,
    normalize_offer_side,
    resolve_offer_expiry_for_pricing,
    resolve_quote_price_for_pricing,
)


def keyring_yaml_path_for_market(program: ProgramConfig, market: MarketConfig) -> str:
    signer_key = program.signer_key_registry.get(market.signer_key_id)
    return str(signer_key.keyring_yaml_path or "") if signer_key is not None else ""


def default_program_config_path(
    program: ProgramConfig,
    program_path: Path | None = None,
) -> Path:
    return program_path or Path(str(program.home_dir)) / "config" / "program.yaml"


@dataclass(frozen=True, slots=True)
class OfferBuildContext:
    """Shared manual-offer build inputs.

    ``action_side`` is normalized once in ``prepare_offer_build_context`` to ``buy`` or
    ``sell``. Downstream code should use it directly instead of calling
    ``normalize_offer_side`` again.
    """

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
    keyring_yaml_path: str | None = None,
    action_side: str = "sell",
) -> OfferBuildContext:
    pricing = dict(market.pricing or {})
    resolved_quote_asset = resolve_quote_asset_for_offer(
        quote_asset=str(market.quote_asset),
        network=network,
    )
    base_unit_mojo_multiplier = int(
        mojo_multiplier_for_leg(
            pricing,
            "base_unit_mojo_multiplier",
            str(market.base_asset),
        )
    )
    quote_unit_mojo_multiplier = int(
        mojo_multiplier_for_leg(
            pricing,
            "quote_unit_mojo_multiplier",
            str(resolved_quote_asset),
        )
    )
    expiry_unit, expiry_value = resolve_offer_expiry_for_pricing(pricing)
    quote_price = resolve_quote_price_for_pricing(pricing)
    resolved_keyring_yaml_path = (
        keyring_yaml_path_for_market(program, market)
        if keyring_yaml_path is None
        else keyring_yaml_path
    )
    return OfferBuildContext(
        program=program,
        market=market,
        program_path=program_path,
        network=network,
        keyring_yaml_path=resolved_keyring_yaml_path,
        resolved_quote_asset=resolved_quote_asset,
        expiry_unit=expiry_unit,
        expiry_value=int(expiry_value),
        base_unit_mojo_multiplier=base_unit_mojo_multiplier,
        quote_unit_mojo_multiplier=quote_unit_mojo_multiplier,
        quote_price=float(quote_price),
        action_side=normalize_offer_side(action_side),
    )

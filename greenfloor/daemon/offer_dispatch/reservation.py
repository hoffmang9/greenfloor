"""Reservation request shaping for managed offer dispatch."""

from __future__ import annotations

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.offer_assets_bridge import resolve_offer_assets
from greenfloor.core.offer_policy import resolve_quote_price_for_pricing
from greenfloor.core.parallel_reservation_context import ParallelReservationContext
from greenfloor.daemon.market_helpers import _resolve_quote_asset_for_offer


def reservation_wallet_id(program: ProgramConfig) -> str:
    vault = program.vault_config
    if vault is not None:
        launcher_id = str(vault.launcher_id).strip()
        if launcher_id:
            return launcher_id
    return "signer"


def parallel_reservation_context(
    *,
    market: MarketConfig,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    resolved_xch_asset_id: str,
    fee_amount_mojos: int = 0,
) -> ParallelReservationContext:
    pricing = market.pricing or {}
    return ParallelReservationContext(
        base_asset_id=str(resolved_base_asset_id or "").strip(),
        quote_asset_id=str(resolved_quote_asset_id or "").strip(),
        fee_asset_id=str(resolved_xch_asset_id or "").strip(),
        fee_amount_mojos=int(fee_amount_mojos),
        base_unit_mojo_multiplier=int(pricing.get("base_unit_mojo_multiplier", 1000)),
        quote_unit_mojo_multiplier=int(pricing.get("quote_unit_mojo_multiplier", 1000)),
        quote_price=float(resolve_quote_price_for_pricing(dict(market.pricing or {}))),
    )


def resolve_signer_offer_asset_ids_for_reservation(
    *,
    program: ProgramConfig,
    market: MarketConfig,
) -> tuple[str, str, str]:
    quote_asset = _resolve_quote_asset_for_offer(
        quote_asset=str(market.quote_asset),
        network=str(program.app_network),
    )
    resolved_base_asset_id, resolved_quote_asset_id = resolve_offer_assets(
        str(market.base_asset),
        quote_asset,
        program=program,
    )
    resolved_xch_asset_id, _ = resolve_offer_assets("xch", quote_asset, program=program)
    return resolved_base_asset_id, resolved_quote_asset_id, resolved_xch_asset_id

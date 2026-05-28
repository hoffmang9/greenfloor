"""Reservation request shaping for managed offer dispatch."""

from __future__ import annotations

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.planned_action import PlannedAction
from greenfloor.core.cycle import reservation_request_for_managed_offer
from greenfloor.daemon.market_helpers import _normalize_offer_side, _resolve_quote_asset_for_offer
from greenfloor.runtime.offer_publish import resolve_quote_price_for_market
from greenfloor.runtime.offer_runtime import signer_resolve_offer_asset_ids


def reservation_wallet_id(program: ProgramConfig) -> str:
    vault = program.vault_config
    if vault is not None:
        launcher_id = str(vault.launcher_id).strip()
        if launcher_id:
            return launcher_id
    return "signer"


def reservation_request_for_action(
    *,
    market: MarketConfig,
    action: PlannedAction,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    fee_asset_id: str,
    fee_amount_mojos: int,
) -> dict[str, int]:
    pricing = market.pricing or {}
    base_multiplier = int(pricing.get("base_unit_mojo_multiplier", 1000))
    quote_multiplier = int(pricing.get("quote_unit_mojo_multiplier", 1000))
    return reservation_request_for_managed_offer(
        side=_normalize_offer_side(action.side),
        size_base_units=int(action.size),
        base_asset_id=str(resolved_base_asset_id or "").strip(),
        quote_asset_id=str(resolved_quote_asset_id or "").strip(),
        base_unit_mojo_multiplier=base_multiplier,
        quote_unit_mojo_multiplier=quote_multiplier,
        quote_price=float(resolve_quote_price_for_market(market)),
        fee_asset_id=str(fee_asset_id or "").strip(),
        fee_amount_mojos=int(fee_amount_mojos),
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
    resolved_base_asset_id, resolved_quote_asset_id = signer_resolve_offer_asset_ids(
        program=program,
        base_asset_id=str(market.base_asset).strip(),
        quote_asset_id=str(quote_asset).strip(),
    )
    resolved_xch_asset_id, _ = signer_resolve_offer_asset_ids(
        program=program,
        base_asset_id="xch",
        quote_asset_id=str(quote_asset).strip(),
    )
    return resolved_base_asset_id, resolved_quote_asset_id, resolved_xch_asset_id

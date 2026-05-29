"""Runtime orchestration for unified offer-action builds."""

from __future__ import annotations

from greenfloor.adapters import offer_action
from greenfloor.config.models import prepare_signer_runtime
from greenfloor.core.offer_action import (
    OfferActionRequest,
    OfferActionResult,
    build_action_request,
    to_create_phase_outcome,
)
from greenfloor.core.offer_assets_bridge import resolve_offer_assets
from greenfloor.runtime.offer_build_context import OfferBuildContext

__all__ = [
    "action_request_from_context",
    "build_bls_create_phase_from_build_context",
    "build_bls_offer_from_build_context",
    "resolve_action_assets_for_build_context",
]


def action_request_from_context(
    build_ctx: OfferBuildContext,
    *,
    size_base_units: int,
    action_side: str | None = None,
    quote_price: float | None = None,
    offer_coin_ids: list[str] | None = None,
    split_input_coins: bool = True,
    broadcast_split: bool = True,
    resolved_base_asset_id: str | None = None,
    resolved_quote_asset_id: str | None = None,
) -> OfferActionRequest:
    """Build an action request from shared offer build context."""
    market = build_ctx.market
    return build_action_request(
        receive_address=str(market.receive_address or ""),
        base_asset=str(resolved_base_asset_id or market.base_asset),
        quote_asset=str(resolved_quote_asset_id or build_ctx.resolved_quote_asset),
        pricing=dict(market.pricing or {}),
        size_base_units=int(size_base_units),
        action_side=str(action_side or build_ctx.action_side),
        quote_price=float(quote_price if quote_price is not None else build_ctx.quote_price),
        split_input_coins=split_input_coins,
        broadcast_split=broadcast_split,
        offer_coin_ids=offer_coin_ids,
    )


def resolve_action_assets_for_build_context(
    build_ctx: OfferBuildContext,
) -> tuple[str, str]:
    """Resolve market symbols to canonical asset ids for local BLS offer-action builds."""
    return resolve_offer_assets(
        str(build_ctx.market.base_asset),
        str(build_ctx.resolved_quote_asset),
        program=build_ctx.program,
    )


def build_bls_offer_from_build_context(
    build_ctx: OfferBuildContext,
    *,
    size_base_units: int,
    action_side: str | None = None,
    quote_price: float | None = None,
    offer_coin_ids: list[str] | None = None,
) -> OfferActionResult:
    """Build an offer via the Rust engine BLS path."""
    market = build_ctx.market
    key_id = str(market.signer_key_id or "").strip()
    if not key_id:
        raise ValueError("missing_key_id")
    resolved_base, resolved_quote = resolve_action_assets_for_build_context(build_ctx)
    request = action_request_from_context(
        build_ctx,
        size_base_units=size_base_units,
        action_side=action_side,
        quote_price=quote_price,
        offer_coin_ids=offer_coin_ids,
        split_input_coins=False,
        broadcast_split=False,
        resolved_base_asset_id=resolved_base,
        resolved_quote_asset_id=resolved_quote,
    )
    return offer_action.build_bls_offer_for_action(
        network=str(build_ctx.network),
        key_id=key_id,
        request=request,
        config_path=prepare_signer_runtime(build_ctx.program),
    )


def build_bls_create_phase_from_build_context(
    build_ctx: OfferBuildContext,
    *,
    size_base_units: int,
    action_side: str | None = None,
    quote_price: float | None = None,
    offer_coin_ids: list[str] | None = None,
):
    """Build a BLS offer and map to create-phase outcome fields."""
    result = build_bls_offer_from_build_context(
        build_ctx,
        size_base_units=size_base_units,
        action_side=action_side,
        quote_price=quote_price,
        offer_coin_ids=offer_coin_ids,
    )
    return to_create_phase_outcome(
        result,
        action_side=str(action_side or build_ctx.action_side),
    )

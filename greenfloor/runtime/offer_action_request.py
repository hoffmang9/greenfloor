"""Build offer-action requests from shared offer build context."""

from __future__ import annotations

from greenfloor.core.offer_action import OfferActionRequest, build_action_request
from greenfloor.runtime.offer_build_context import OfferBuildContext

__all__ = ["action_request_from_context"]


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

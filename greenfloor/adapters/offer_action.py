"""Rust-kernel IO for unified offer-action build."""

from __future__ import annotations

from greenfloor.core.kernel_bridge import import_kernel
from greenfloor.core.offer_action import (
    OfferActionRequest,
    OfferActionResult,
    parse_action_result,
)
from greenfloor.runtime.offer_action_request import action_request_from_context
from greenfloor.runtime.offer_build_context import OfferBuildContext

__all__ = [
    "build_bls_offer_for_action",
    "build_bls_offer_from_build_context",
    "build_signer_offer_for_action",
]


def build_signer_offer_for_action(
    config_path: str,
    request: OfferActionRequest,
) -> OfferActionResult:
    kernel = import_kernel()
    result = kernel.build_signer_offer_for_action(str(config_path), dict(request))
    return parse_action_result(result)


def build_bls_offer_for_action(
    *,
    network: str,
    key_id: str,
    request: OfferActionRequest,
) -> OfferActionResult:
    kernel = import_kernel()
    result = kernel.build_bls_offer_for_action_key(str(network), str(key_id), dict(request))
    return parse_action_result(result)


def build_bls_offer_from_build_context(
    build_ctx: OfferBuildContext,
    *,
    size_base_units: int,
    action_side: str | None = None,
    quote_price: float | None = None,
    offer_coin_ids: list[str] | None = None,
) -> OfferActionResult:
    """Build an offer via the Rust kernel BLS path."""
    market = build_ctx.market
    key_id = str(market.signer_key_id or "").strip()
    if not key_id:
        raise ValueError("missing_key_id")
    request = action_request_from_context(
        build_ctx,
        size_base_units=size_base_units,
        action_side=action_side,
        quote_price=quote_price,
        offer_coin_ids=offer_coin_ids,
        split_input_coins=False,
        broadcast_split=False,
    )
    return build_bls_offer_for_action(
        network=str(build_ctx.network),
        key_id=key_id,
        request=request,
    )

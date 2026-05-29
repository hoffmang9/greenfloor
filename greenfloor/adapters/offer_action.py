"""Unified Rust-kernel offer build for market actions (signer + local BLS)."""

from __future__ import annotations

import datetime as dt
from typing import Any

from greenfloor.config.models import MarketConfig
from greenfloor.core.kernel_bridge import import_kernel
from greenfloor.runtime.offer_build_context import OfferBuildContext


def action_request_from_build_context(
    build_ctx: OfferBuildContext,
    *,
    size_base_units: int,
    action_side: str | None = None,
    quote_price: float | None = None,
    offer_coin_ids: list[str] | None = None,
    split_input_coins: bool = True,
    broadcast_split: bool = True,
) -> dict[str, Any]:
    """Shape a kernel ``BuildOfferForActionRequest`` dict from offer build context."""
    market = build_ctx.market
    receive_address = str(market.receive_address or "").strip()
    if not receive_address:
        raise ValueError("market.receive_address is required for offer build")
    return {
        "receive_address": receive_address,
        "base_asset": str(market.base_asset),
        "quote_asset": str(build_ctx.resolved_quote_asset),
        "size_base_units": int(size_base_units),
        "action_side": str(action_side or build_ctx.action_side),
        "pricing": dict(market.pricing or {}),
        "quote_price": float(quote_price if quote_price is not None else build_ctx.quote_price),
        "split_input_coins": bool(split_input_coins),
        "broadcast_split": bool(broadcast_split),
        "offer_coin_ids": list(offer_coin_ids or []),
    }


def action_request_from_market(
    *,
    market: MarketConfig,
    resolved_quote_asset: str,
    size_base_units: int,
    quote_price: float,
    action_side: str,
    offer_coin_ids: list[str] | None = None,
    split_input_coins: bool = True,
    broadcast_split: bool = True,
) -> dict[str, Any]:
    receive_address = str(market.receive_address or "").strip()
    if not receive_address:
        raise ValueError("market.receive_address is required for offer build")
    return {
        "receive_address": receive_address,
        "base_asset": str(market.base_asset),
        "quote_asset": str(resolved_quote_asset),
        "size_base_units": int(size_base_units),
        "action_side": str(action_side),
        "pricing": dict(market.pricing or {}),
        "quote_price": float(quote_price),
        "split_input_coins": bool(split_input_coins),
        "broadcast_split": bool(broadcast_split),
        "offer_coin_ids": list(offer_coin_ids or []),
    }


def _coerce_action_result(payload: object) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise TypeError("offer action kernel returned non-dict result")
    error = payload.get("error")
    if error:
        raise RuntimeError(str(error))
    offer_text = str(payload.get("offer_text", "")).strip()
    if not offer_text.startswith("offer1"):
        raise RuntimeError("offer_action_failed:missing_offer_text")
    return dict(payload)


def _expires_at_iso(expires_at_unix: int) -> str:
    return dt.datetime.fromtimestamp(int(expires_at_unix), tz=dt.UTC).isoformat()


def build_signer_offer_for_action(config_path: str, request: dict[str, Any]) -> dict[str, Any]:
    kernel = import_kernel()
    result = kernel.build_signer_offer_for_action(str(config_path), dict(request))
    return _coerce_action_result(result)


def build_bls_offer_for_action(
    *,
    network: str,
    key_id: str,
    request: dict[str, Any],
) -> dict[str, Any]:
    kernel = import_kernel()
    result = kernel.build_bls_offer_for_action_key(str(network), str(key_id), dict(request))
    return _coerce_action_result(result)


def build_offer_text_from_build_context(
    build_ctx: OfferBuildContext,
    *,
    size_base_units: int,
    action_side: str | None = None,
    quote_price: float | None = None,
    offer_coin_ids: list[str] | None = None,
) -> str:
    """Build an offer1... string via the Rust kernel BLS path."""
    market = build_ctx.market
    key_id = str(market.signer_key_id or "").strip()
    if not key_id:
        raise ValueError("missing_key_id")
    request = action_request_from_build_context(
        build_ctx,
        size_base_units=size_base_units,
        action_side=action_side,
        quote_price=quote_price,
        offer_coin_ids=offer_coin_ids,
        split_input_coins=False,
        broadcast_split=False,
    )
    result = build_bls_offer_for_action(
        network=str(build_ctx.network),
        key_id=key_id,
        request=request,
    )
    return str(result["offer_text"])


def create_offer_outcome_from_action_result(
    result: dict[str, Any],
    *,
    action_side: str,
) -> dict[str, Any]:
    """Map kernel action result to signer/local create-phase fields."""
    expires_at_unix = int(result.get("expires_at_unix", 0))
    create_result = result.get("create_result")
    extra: dict[str, Any] = {}
    execution_mode = str(result.get("execution_mode", "")).strip()
    if execution_mode:
        extra["execution_mode"] = execution_mode
    return {
        "offer_text": str(result["offer_text"]),
        "expires_at": _expires_at_iso(expires_at_unix) if expires_at_unix > 0 else "",
        "side": str(result.get("side", action_side)),
        "offer_amount": int(result.get("offer_amount", 0)),
        "request_amount": int(result.get("request_amount", 0)),
        "execution_mode": execution_mode,
        "create_result": dict(create_result) if isinstance(create_result, dict) else {},
        "extra": extra,
    }

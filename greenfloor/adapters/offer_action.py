"""Unified Rust-kernel offer build for market actions (signer + local BLS)."""

from __future__ import annotations

import datetime as dt
from dataclasses import dataclass
from typing import Any, TypedDict

from greenfloor.core.kernel_bridge import import_kernel
from greenfloor.runtime.offer_build_context import OfferBuildContext

__all__ = [
    "OfferActionRequest",
    "OfferActionResult",
    "OfferCreatePhaseOutcome",
    "action_request_from_context",
    "build_action_request",
    "build_bls_offer_for_action",
    "build_bls_offer_from_build_context",
    "build_offer_text_from_build_context",
    "build_signer_offer_for_action",
    "legacy_action_request_from_payload",
    "to_create_phase_outcome",
]


class OfferActionRequest(TypedDict):
    receive_address: str
    base_asset: str
    quote_asset: str
    size_base_units: int
    action_side: str
    pricing: dict[str, Any]
    quote_price: float
    split_input_coins: bool
    broadcast_split: bool
    offer_coin_ids: list[str]


class OfferActionResult(TypedDict):
    offer_text: str
    side: str
    expires_at_unix: int
    offer_amount: int
    request_amount: int
    execution_mode: str
    create_result: dict[str, Any]


@dataclass(frozen=True, slots=True)
class OfferCreatePhaseOutcome:
    offer_text: str
    expires_at: str
    side: str
    offer_amount: int
    request_amount: int
    execution_mode: str
    create_result: dict[str, Any]


def build_action_request(
    *,
    receive_address: str,
    base_asset: str,
    quote_asset: str,
    pricing: dict[str, Any],
    size_base_units: int,
    action_side: str,
    quote_price: float,
    split_input_coins: bool = True,
    broadcast_split: bool = True,
    offer_coin_ids: list[str] | None = None,
) -> OfferActionRequest:
    """Shape a kernel ``BuildOfferForActionRequest`` dict."""
    address = str(receive_address or "").strip()
    if not address:
        raise ValueError("market.receive_address is required for offer build")
    return OfferActionRequest(
        receive_address=address,
        base_asset=str(base_asset),
        quote_asset=str(quote_asset),
        size_base_units=int(size_base_units),
        action_side=str(action_side),
        pricing=dict(pricing or {}),
        quote_price=float(quote_price),
        split_input_coins=bool(split_input_coins),
        broadcast_split=bool(broadcast_split),
        offer_coin_ids=list(offer_coin_ids or []),
    )


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


def legacy_action_request_from_payload(payload: dict[str, Any]) -> OfferActionRequest:
    """Map legacy offer_builder stdin payloads to the unified action request."""
    receive_address = str(payload.get("receive_address", "")).strip()
    key_id = str(payload.get("key_id", "")).strip()
    network = str(payload.get("network", "")).strip()
    keyring_yaml_path = str(payload.get("keyring_yaml_path", "")).strip()
    size_base_units = int(payload.get("size_base_units", 0))
    quote_price_quote_per_base = float(payload.get("quote_price_quote_per_base", 0.0))
    base_unit_mojo_multiplier = int(payload.get("base_unit_mojo_multiplier", 0))
    quote_unit_mojo_multiplier = int(payload.get("quote_unit_mojo_multiplier", 0))
    if not receive_address:
        raise ValueError("missing_receive_address")
    if size_base_units <= 0:
        raise ValueError("invalid_size_base_units")
    if not key_id:
        raise ValueError("missing_key_id")
    if not network:
        raise ValueError("missing_network")
    if not keyring_yaml_path:
        raise ValueError("missing_keyring_yaml_path")
    if quote_price_quote_per_base <= 0:
        raise ValueError("invalid_quote_price_quote_per_base")
    if base_unit_mojo_multiplier <= 0:
        raise ValueError("invalid_base_unit_mojo_multiplier")
    if quote_unit_mojo_multiplier <= 0:
        raise ValueError("invalid_quote_unit_mojo_multiplier")

    asset_id = str(payload.get("asset_id", "xch")).strip().lower() or "xch"
    quote_asset = str(payload.get("quote_asset", "xch")).strip().lower() or "xch"
    if quote_asset not in {"xch", "txch", "1"} and len(quote_asset) != 64:
        raise ValueError("invalid_quote_asset_id")

    return build_action_request(
        receive_address=receive_address,
        base_asset=asset_id,
        quote_asset=quote_asset,
        pricing={
            "base_unit_mojo_multiplier": base_unit_mojo_multiplier,
            "quote_unit_mojo_multiplier": quote_unit_mojo_multiplier,
        },
        size_base_units=size_base_units,
        action_side=str(payload.get("side", "sell")),
        quote_price=quote_price_quote_per_base,
        split_input_coins=bool(payload.get("split_input_coins", True)),
        broadcast_split=bool(payload.get("broadcast_split", False)),
        offer_coin_ids=[
            str(value).strip().lower()
            for value in (payload.get("offer_coin_ids") or [])
            if str(value).strip()
        ],
    )


def _parse_action_result(payload: object) -> OfferActionResult:
    if not isinstance(payload, dict):
        raise TypeError("offer action kernel returned non-dict result")
    offer_text = str(payload.get("offer_text", "")).strip()
    if not offer_text.startswith("offer1"):
        raise RuntimeError("offer_action_failed:missing_offer_text")
    return OfferActionResult(
        offer_text=offer_text,
        side=str(payload.get("side", "")),
        expires_at_unix=int(payload.get("expires_at_unix", 0)),
        offer_amount=int(payload.get("offer_amount", 0)),
        request_amount=int(payload.get("request_amount", 0)),
        execution_mode=str(payload.get("execution_mode", "")),
        create_result=dict(payload["create_result"])
        if isinstance(payload.get("create_result"), dict)
        else {},
    )


def expires_at_iso_from_unix(expires_at_unix: int) -> str:
    if expires_at_unix <= 0:
        return ""
    return dt.datetime.fromtimestamp(int(expires_at_unix), tz=dt.UTC).isoformat()


def to_create_phase_outcome(
    result: OfferActionResult,
    *,
    action_side: str,
) -> OfferCreatePhaseOutcome:
    """Map kernel action result to signer/local create-phase fields."""
    expires_at_unix = int(result.get("expires_at_unix", 0))
    execution_mode = str(result.get("execution_mode", "")).strip()
    return OfferCreatePhaseOutcome(
        offer_text=str(result["offer_text"]),
        expires_at=expires_at_iso_from_unix(expires_at_unix),
        side=str(result.get("side", action_side)),
        offer_amount=int(result.get("offer_amount", 0)),
        request_amount=int(result.get("request_amount", 0)),
        execution_mode=execution_mode,
        create_result=dict(result.get("create_result") or {}),
    )


def build_signer_offer_for_action(
    config_path: str,
    request: OfferActionRequest,
) -> OfferActionResult:
    kernel = import_kernel()
    result = kernel.build_signer_offer_for_action(str(config_path), dict(request))
    return _parse_action_result(result)


def build_bls_offer_for_action(
    *,
    network: str,
    key_id: str,
    request: OfferActionRequest,
) -> OfferActionResult:
    kernel = import_kernel()
    result = kernel.build_bls_offer_for_action_key(str(network), str(key_id), dict(request))
    return _parse_action_result(result)


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


def build_offer_text_from_build_context(
    build_ctx: OfferBuildContext,
    *,
    size_base_units: int,
    action_side: str | None = None,
    quote_price: float | None = None,
    offer_coin_ids: list[str] | None = None,
) -> str:
    """Build an offer1... string via the Rust kernel BLS path."""
    return str(
        build_bls_offer_from_build_context(
            build_ctx,
            size_base_units=size_base_units,
            action_side=action_side,
            quote_price=quote_price,
            offer_coin_ids=offer_coin_ids,
        )["offer_text"]
    )

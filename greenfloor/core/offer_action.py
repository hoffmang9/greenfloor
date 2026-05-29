"""Typed contracts and pure helpers for unified offer-action build requests."""

from __future__ import annotations

import datetime as dt
import time
from dataclasses import dataclass
from typing import Any, TypedDict

__all__ = [
    "OfferActionRequest",
    "OfferActionResult",
    "OfferCreatePhaseOutcome",
    "build_action_request",
    "expires_at_iso_from_build_context",
    "expires_at_iso_from_unix",
    "legacy_action_request_from_payload",
    "parse_action_result",
    "to_create_phase_outcome",
    "validate_legacy_offer_payload",
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
    """Shape a engine ``BuildOfferForActionRequest`` dict."""
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


def validate_legacy_offer_payload(payload: dict[str, Any]) -> None:
    """Validate legacy offer_builder stdin payloads before action request shaping."""
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

    quote_asset = str(payload.get("quote_asset", "xch")).strip().lower() or "xch"
    if quote_asset not in {"xch", "txch", "1"} and len(quote_asset) != 64:
        raise ValueError("invalid_quote_asset_id")


def legacy_action_request_from_payload(payload: dict[str, Any]) -> OfferActionRequest:
    """Map a validated legacy offer_builder payload to an action request."""
    validate_legacy_offer_payload(payload)
    asset_id = str(payload.get("asset_id", "xch")).strip().lower() or "xch"
    quote_asset = str(payload.get("quote_asset", "xch")).strip().lower() or "xch"
    return build_action_request(
        receive_address=str(payload.get("receive_address", "")).strip(),
        base_asset=asset_id,
        quote_asset=quote_asset,
        pricing={
            "base_unit_mojo_multiplier": int(payload.get("base_unit_mojo_multiplier", 0)),
            "quote_unit_mojo_multiplier": int(payload.get("quote_unit_mojo_multiplier", 0)),
        },
        size_base_units=int(payload.get("size_base_units", 0)),
        action_side=str(payload.get("side", "sell")),
        quote_price=float(payload.get("quote_price_quote_per_base", 0.0)),
        split_input_coins=bool(payload.get("split_input_coins", True)),
        broadcast_split=bool(payload.get("broadcast_split", False)),
        offer_coin_ids=[
            str(value).strip().lower()
            for value in (payload.get("offer_coin_ids") or [])
            if str(value).strip()
        ],
    )


def parse_action_result(payload: object) -> OfferActionResult:
    if not isinstance(payload, dict):
        raise TypeError("offer action engine returned non-dict result")
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


def expires_at_iso_from_build_context(*, expiry_unit: str, expiry_value: int) -> str:
    """ISO expiry from build-context pricing (minutes-only contract)."""
    del expiry_unit
    value = int(expiry_value)
    if value <= 0:
        return ""
    return expires_at_iso_from_unix(int(time.time()) + value * 60)


def to_create_phase_outcome(
    result: OfferActionResult,
    *,
    action_side: str,
) -> OfferCreatePhaseOutcome:
    """Map engine action result to signer/local create-phase fields."""
    return OfferCreatePhaseOutcome(
        offer_text=result["offer_text"],
        expires_at=expires_at_iso_from_unix(result["expires_at_unix"]),
        side=result["side"] or str(action_side),
        offer_amount=result["offer_amount"],
        request_amount=result["request_amount"],
        execution_mode=result["execution_mode"].strip(),
        create_result=dict(result["create_result"]),
    )

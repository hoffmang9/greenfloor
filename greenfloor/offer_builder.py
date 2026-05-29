"""Legacy offer-builder entry point; ``build_offer`` uses the Rust kernel BLS action path."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.native_offer import encode_offer_from_spend_bundle_hex
from greenfloor.adapters.offer_action import build_bls_offer_for_action


def _validate_coin_backed_payload(payload: dict[str, Any]) -> None:
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


def _action_request_from_legacy_payload(payload: dict[str, Any]) -> dict[str, Any]:
    _validate_coin_backed_payload(payload)
    asset_id = str(payload.get("asset_id", "xch")).strip().lower() or "xch"
    quote_asset = str(payload.get("quote_asset", "xch")).strip().lower() or "xch"
    if quote_asset not in {"xch", "txch", "1"} and len(quote_asset) != 64:
        raise ValueError("invalid_quote_asset_id")
    return {
        "receive_address": str(payload.get("receive_address", "")).strip(),
        "base_asset": asset_id,
        "quote_asset": quote_asset,
        "size_base_units": int(payload.get("size_base_units", 0)),
        "action_side": str(payload.get("side", "sell")),
        "pricing": {
            "base_unit_mojo_multiplier": int(payload.get("base_unit_mojo_multiplier", 0)),
            "quote_unit_mojo_multiplier": int(payload.get("quote_unit_mojo_multiplier", 0)),
        },
        "quote_price": float(payload.get("quote_price_quote_per_base", 0.0)),
        "split_input_coins": bool(payload.get("split_input_coins", True)),
        "broadcast_split": bool(payload.get("broadcast_split", False)),
        "offer_coin_ids": [
            str(value).strip().lower()
            for value in (payload.get("offer_coin_ids") or [])
            if str(value).strip()
        ],
    }


def _build_coin_backed_spend_bundle_hex(payload: dict[str, Any]) -> str:
    from greenfloor.adapters.bls_signing import build_signed_spend_bundle
    from greenfloor.core.offer_policy import compute_signer_offer_leg_amounts

    _validate_coin_backed_payload(payload)
    asset_id = str(payload.get("asset_id", "xch")).strip().lower() or "xch"
    quote_asset = str(payload.get("quote_asset", "xch")).strip().lower() or "xch"
    if quote_asset not in {"xch", "txch", "1"} and len(quote_asset) != 64:
        raise ValueError("invalid_quote_asset_id")

    leg = compute_signer_offer_leg_amounts(
        size_base_units=int(payload.get("size_base_units", 0)),
        quote_price=float(payload.get("quote_price_quote_per_base", 0.0)),
        resolved_base_asset_id=asset_id,
        resolved_quote_asset_id=quote_asset,
        action_side="sell",
        pricing={
            "base_unit_mojo_multiplier": int(payload.get("base_unit_mojo_multiplier", 0)),
            "quote_unit_mojo_multiplier": int(payload.get("quote_unit_mojo_multiplier", 0)),
        },
    )

    raw_offer_coin_ids = payload.get("offer_coin_ids", [])
    offer_coin_ids = (
        [str(value).strip().lower() for value in raw_offer_coin_ids if str(value).strip()]
        if isinstance(raw_offer_coin_ids, list)
        else []
    )
    result = build_signed_spend_bundle(
        {
            "key_id": str(payload.get("key_id", "")).strip(),
            "network": str(payload.get("network", "")).strip(),
            "receive_address": str(payload.get("receive_address", "")).strip(),
            "keyring_yaml_path": str(payload.get("keyring_yaml_path", "")).strip(),
            "asset_id": asset_id,
            "dry_run": bool(payload.get("dry_run", False)),
            "plan": {
                "op_type": "offer",
                "offer_asset_id": asset_id,
                "offer_amount": int(leg.offer_amount_mojos),
                "request_asset_id": quote_asset,
                "request_amount": int(leg.request_amount_mojos),
                "offer_coin_ids": offer_coin_ids,
            },
        }
    )
    if result.get("status") != "executed":
        raise RuntimeError(str(result.get("reason", "coin_backed_signing_failed")))
    spend_bundle_hex = str(result.get("spend_bundle_hex", "")).strip()
    if not spend_bundle_hex:
        raise RuntimeError("missing_spend_bundle_hex")
    return spend_bundle_hex


def _build_offer(payload: dict[str, Any]) -> str:
    spend_bundle_hex = str(payload.get("spend_bundle_hex", "")).strip()
    if spend_bundle_hex:
        raw_hex = (
            spend_bundle_hex[2:] if spend_bundle_hex.lower().startswith("0x") else spend_bundle_hex
        )
        return encode_offer_from_spend_bundle_hex(raw_hex)
    request = _action_request_from_legacy_payload(payload)
    result = build_bls_offer_for_action(
        network=str(payload.get("network", "")).strip(),
        key_id=str(payload.get("key_id", "")).strip(),
        request=request,
    )
    return str(result["offer_text"])


def build_offer(payload: dict[str, Any]) -> str:
    """Build an offer1... string from payload. Raises on failure."""
    return _build_offer(payload)

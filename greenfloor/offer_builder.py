from __future__ import annotations

from typing import Any


def _import_sdk() -> Any:
    import chia_wallet_sdk as sdk  # type: ignore

    return sdk


def _build_coin_backed_spend_bundle_hex(payload: dict[str, Any]) -> str:
    from greenfloor.signing import build_signed_spend_bundle

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
    raw_offer_coin_ids = payload.get("offer_coin_ids", [])
    offer_coin_ids = (
        [str(value).strip().lower() for value in raw_offer_coin_ids if str(value).strip()]
        if isinstance(raw_offer_coin_ids, list)
        else []
    )
    if quote_asset in {"xch", "txch", "1"}:
        request_asset_id = quote_asset
    else:
        if len(quote_asset) != 64:
            raise ValueError("invalid_quote_asset_id")
        request_asset_id = quote_asset

    offer_amount = int(size_base_units) * int(base_unit_mojo_multiplier)
    request_amount = int(
        round(
            float(size_base_units)
            * float(quote_price_quote_per_base)
            * float(quote_unit_mojo_multiplier)
        )
    )
    if offer_amount <= 0:
        raise ValueError("invalid_offer_amount")
    if request_amount <= 0:
        raise ValueError("invalid_request_amount")

    result = build_signed_spend_bundle(
        {
            "key_id": key_id,
            "network": network,
            "receive_address": receive_address,
            "keyring_yaml_path": keyring_yaml_path,
            "asset_id": asset_id,
            "dry_run": bool(payload.get("dry_run", False)),
            "cloud_wallet_base_url": str(payload.get("cloud_wallet_base_url", "")).strip(),
            "cloud_wallet_user_key_id": str(payload.get("cloud_wallet_user_key_id", "")).strip(),
            "cloud_wallet_private_key_pem_path": str(
                payload.get("cloud_wallet_private_key_pem_path", "")
            ).strip(),
            "cloud_wallet_vault_id": str(payload.get("cloud_wallet_vault_id", "")).strip(),
            "cloud_wallet_kms_key_id": str(payload.get("cloud_wallet_kms_key_id", "")).strip(),
            "cloud_wallet_kms_region": str(payload.get("cloud_wallet_kms_region", "")).strip(),
            "cloud_wallet_kms_public_key_hex": str(
                payload.get("cloud_wallet_kms_public_key_hex", "")
            ).strip(),
            "signing_entrypoint": "build_signed_spend_bundle",
            "plan": {
                "op_type": "offer",
                "offer_asset_id": asset_id,
                "offer_amount": offer_amount,
                "request_asset_id": request_asset_id,
                "request_amount": request_amount,
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


def _build_offer(payload: dict[str, Any], sdk: Any) -> str:
    spend_bundle_hex = str(payload.get("spend_bundle_hex", "")).strip()
    if not spend_bundle_hex:
        spend_bundle_hex = _build_coin_backed_spend_bundle_hex(payload)
    spend_bundle = sdk.SpendBundle.from_bytes(sdk.from_hex(spend_bundle_hex))
    return str(sdk.encode_offer(spend_bundle))


def build_offer(payload: dict[str, Any]) -> str:
    """Build an offer text string from payload. Raises on failure."""
    sdk = _import_sdk()
    return _build_offer(payload, sdk)


def build_offer_text(payload: dict[str, Any]) -> str:
    """Build an offer1... string via in-process offer construction."""
    return build_offer(payload)

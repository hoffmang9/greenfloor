"""BLS keyring signing: thin Python wrappers over Rust kernel BLS paths.

Offer, mixed-split, XCH split/combine spend bundles and key loading run in the
``greenfloor-signer`` crate. Vault KMS uses ``greenfloor.adapters.rust_signer``.
"""

from __future__ import annotations

from typing import Any

from greenfloor.core.kernel_bridge import import_kernel
from greenfloor.hex_utils import canonical_is_xch


def _hex_to_bytes(value: str) -> bytes:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if len(raw) % 2:
        raw = f"0{raw}"
    return bytes.fromhex(raw)


def _load_master_private_key(
    keyring_yaml_path: str, key_id: str
) -> tuple[bytes | None, str | None]:
    _ = keyring_yaml_path
    try:
        kernel = import_kernel()
        result = kernel.load_bls_master_sk(str(key_id).strip())
    except Exception as exc:
        return None, f"greenfloor_signer_import_error:{exc}"
    if not isinstance(result, dict):
        return None, "invalid_load_bls_master_sk_response"
    error = result.get("error")
    if error:
        return None, str(error)
    raw = result.get("master_sk_bytes")
    if not isinstance(raw, bytes | bytearray | memoryview):
        return None, "missing_master_sk_bytes"
    return bytes(raw), None


def _call_signer_build(
    method_name: str,
    network: str,
    master_sk_bytes: bytes,
    request: dict[str, Any],
) -> tuple[str | None, str | None]:
    try:
        kernel = import_kernel()
        build = getattr(kernel, method_name)
    except Exception as exc:
        return None, f"greenfloor_signer_import_error:{exc}"
    try:
        result = build(network, master_sk_bytes, request)
    except Exception as exc:
        return None, f"{method_name}_error:{exc}"
    if not isinstance(result, dict):
        return None, f"invalid_{method_name}_response"
    error = result.get("error")
    if error:
        return None, str(error)
    spend_bundle_hex = result.get("spend_bundle_hex")
    if not isinstance(spend_bundle_hex, str) or not spend_bundle_hex.strip():
        return None, "missing_spend_bundle_hex"
    return spend_bundle_hex, None


def _coin_id_set(raw_values: Any) -> set[str]:
    if not isinstance(raw_values, list):
        return set()
    values: set[str] = set()
    for value in raw_values:
        raw = str(value).strip().lower()
        if raw.startswith("0x"):
            raw = raw[2:]
        if raw and all(ch in "0123456789abcdef" for ch in raw):
            values.add(raw)
    return values


def _build_mixed_split_spend_bundle(payload: dict[str, Any]) -> tuple[str | None, str | None]:
    key_id = str(payload.get("key_id", "")).strip()
    network = str(payload.get("network", "")).strip()
    receive_address = str(payload.get("receive_address", "")).strip()
    keyring_yaml_path = str(payload.get("keyring_yaml_path", "")).strip()
    asset_id = str(payload.get("asset_id", "")).strip().lower()
    if not key_id or not network or not receive_address:
        return None, "missing_key_or_network_or_address"
    if not keyring_yaml_path:
        return None, "missing_keyring_yaml_path"
    if not asset_id:
        return None, "missing_asset_id"

    raw_outputs = payload.get("output_amounts_base_units", [])
    if not isinstance(raw_outputs, list) or not raw_outputs:
        return None, "missing_output_amounts"
    output_amounts: list[int] = []
    for value in raw_outputs:
        amount = int(value)
        if amount <= 0:
            return None, "invalid_output_amount"
        output_amounts.append(amount)
    allow_sub_cat_output = bool(payload.get("allow_sub_cat_output", False))
    fee_mojos = int(payload.get("fee_mojos", 0))
    if fee_mojos < 0:
        return None, "invalid_fee_mojos"

    master_private_key, key_error = _load_master_private_key(keyring_yaml_path, key_id)
    if key_error:
        return None, key_error
    if master_private_key is None:
        return None, "key_secrets_unavailable"

    request = {
        "receive_address": receive_address,
        "asset_id": asset_id,
        "output_amounts": output_amounts,
        "coin_ids": sorted(_coin_id_set(payload.get("selected_coin_ids", []))),
        "allow_sub_cat_output": allow_sub_cat_output,
        "fee_mojos": fee_mojos,
    }
    return _call_signer_build(
        "build_bls_mixed_split",
        network,
        bytes(master_private_key),
        request,
    )


def _broadcast_bls_spend_bundle_rust(*, network: str, spend_bundle_hex: str) -> dict[str, Any]:
    try:
        kernel = import_kernel()
        result = kernel.broadcast_bls_spend_bundle(network, spend_bundle_hex)
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"greenfloor_signer_import_error:{exc}",
            "operation_id": None,
        }
    if not isinstance(result, dict):
        return {
            "status": "skipped",
            "reason": "invalid_broadcast_response",
            "operation_id": None,
        }
    return {
        "status": str(result.get("status", "skipped")),
        "reason": str(result.get("reason", "unknown")),
        "operation_id": result.get("operation_id"),
    }


def sign_and_broadcast_mixed_split(payload: dict[str, Any]) -> dict[str, Any]:
    spend_bundle_hex, error = _build_mixed_split_spend_bundle(payload)
    if spend_bundle_hex is None:
        return {"status": "skipped", "reason": f"signing_failed:{error}", "operation_id": None}
    return _broadcast_bls_spend_bundle_rust(
        network=str(payload.get("network", "")).strip(),
        spend_bundle_hex=spend_bundle_hex,
    )


def build_signed_spend_bundle(payload: dict[str, Any]) -> dict[str, Any]:
    """Build a signed spend bundle: coin discovery -> selection -> signing."""
    key_id = str(payload.get("key_id", "")).strip()
    network = str(payload.get("network", "")).strip()
    receive_address = str(payload.get("receive_address", "")).strip()
    keyring_yaml_path = str(payload.get("keyring_yaml_path", "")).strip()
    asset_id = str(payload.get("asset_id", "")).strip().lower()

    if not key_id or not network or not receive_address:
        return {"status": "skipped", "reason": "missing_key_or_network_or_address"}
    if not keyring_yaml_path:
        return {"status": "skipped", "reason": "missing_keyring_yaml_path"}
    plan = payload.get("plan") or {}
    if not isinstance(plan, dict):
        return {"status": "skipped", "reason": "missing_plan"}
    op_type = str(plan.get("op_type", "")).strip()

    if op_type == "offer":
        offer_asset_id = str(plan.get("offer_asset_id", asset_id)).strip().lower()
        request_asset_id = str(plan.get("request_asset_id", "")).strip().lower()
        offer_amount = int(plan.get("offer_amount", 0))
        request_amount = int(plan.get("request_amount", 0))
        raw_offer_coin_ids = plan.get("offer_coin_ids", [])
        offer_coin_ids = (
            [str(value).strip().lower() for value in raw_offer_coin_ids if str(value).strip()]
            if isinstance(raw_offer_coin_ids, list)
            else []
        )
        if not request_asset_id:
            return {"status": "skipped", "reason": "missing_request_asset_id"}
        if offer_amount <= 0 or request_amount <= 0:
            return {"status": "skipped", "reason": "signing_failed:invalid_offer_or_request_amount"}
        master_private_key, key_error = _load_master_private_key(keyring_yaml_path, key_id)
        if key_error:
            return {"status": "skipped", "reason": f"signing_failed:{key_error}"}
        if master_private_key is None:
            return {"status": "skipped", "reason": "signing_failed:key_secrets_unavailable"}
        request = {
            "receive_address": receive_address,
            "offer_asset_id": offer_asset_id,
            "offer_amount": offer_amount,
            "request_asset_id": request_asset_id,
            "request_amount": request_amount,
            "offer_coin_ids": offer_coin_ids,
        }
        spend_bundle_hex, error = _call_signer_build(
            "build_bls_offer",
            network,
            bytes(master_private_key),
            request,
        )
        if spend_bundle_hex is None:
            return {"status": "skipped", "reason": f"signing_failed:{error}"}
        return {
            "status": "executed",
            "reason": "signing_success",
            "spend_bundle_hex": spend_bundle_hex,
        }

    if not canonical_is_xch(asset_id):
        return {"status": "skipped", "reason": "asset_not_supported_yet"}

    size_base_units = int(plan.get("size_base_units", 0))
    op_count = int(plan.get("op_count", 0))
    target_total = int(plan.get("target_total_base_units", 0))
    if target_total <= 0 and size_base_units > 0 and op_count > 0:
        target_total = size_base_units * op_count

    master_private_key, key_error = _load_master_private_key(keyring_yaml_path, key_id)
    if key_error:
        return {"status": "skipped", "reason": f"signing_failed:{key_error}"}
    if master_private_key is None:
        return {"status": "skipped", "reason": "signing_failed:key_secrets_unavailable"}

    request = {
        "receive_address": receive_address,
        "op_type": op_type,
        "size_base_units": size_base_units,
        "op_count": op_count,
        "target_total_base_units": target_total,
    }
    spend_bundle_hex, error = _call_signer_build(
        "build_bls_xch_coin_op",
        network,
        bytes(master_private_key),
        request,
    )
    if spend_bundle_hex is None:
        return {"status": "skipped", "reason": f"signing_failed:{error}"}

    return {
        "status": "executed",
        "reason": "signing_success",
        "spend_bundle_hex": spend_bundle_hex,
    }


def sign_and_broadcast(payload: dict[str, Any]) -> dict[str, Any]:
    """Build, sign, and broadcast a spend bundle (daemon coin-op path)."""
    result = build_signed_spend_bundle(payload)
    if result.get("status") != "executed":
        return {
            "status": "skipped",
            "reason": result.get("reason", "signing_failed"),
            "operation_id": None,
        }

    spend_bundle_hex = str(result.get("spend_bundle_hex", ""))
    return _broadcast_bls_spend_bundle_rust(
        network=str(payload.get("network", "")).strip(),
        spend_bundle_hex=spend_bundle_hex,
    )

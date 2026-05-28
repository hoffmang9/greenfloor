"""Thin wrapper around the ``greenfloor_signer`` PyO3 extension."""

from __future__ import annotations

import datetime
from pathlib import Path
from typing import Any

from greenfloor.core.kernel_bridge import import_kernel


def resolve_vault_context(program_path: str) -> dict[str, Any]:
    """Load vault display context from program config via the Rust signer."""
    signer = import_kernel()
    result = signer.resolve_vault_context(str(program_path))
    if not isinstance(result, dict):
        raise TypeError("resolve_vault_context returned non-dict result")
    return result


def build_vault_cat_offer(program_path: str, request_dict: dict[str, Any]) -> dict[str, Any]:
    """Build a vault CAT offer using the canonical Rust signer."""
    signer = import_kernel()
    result = signer.build_vault_cat_offer(str(program_path), request_dict)
    if not isinstance(result, dict):
        raise TypeError("build_vault_cat_offer returned non-dict result")
    return result


def build_mixed_split(program_path: str, request_dict: dict[str, Any]) -> dict[str, Any]:
    """Build (and optionally broadcast) a vault CAT mixed split via the Rust signer."""
    signer = import_kernel()
    result = signer.build_mixed_split(str(program_path), request_dict)
    if not isinstance(result, dict):
        raise TypeError("build_mixed_split returned non-dict result")
    return result


def resolve_offer_asset_ids(program_path: str, base_asset: str, quote_asset: str) -> dict[str, str]:
    """Resolve market symbols or asset ids to canonical offer asset ids."""
    signer = import_kernel()
    result = signer.resolve_offer_asset_ids(str(program_path), base_asset, quote_asset)
    if not isinstance(result, dict):
        raise TypeError("resolve_offer_asset_ids returned non-dict result")
    base_asset_id = str(result.get("base_asset_id", "")).strip()
    quote_asset_id = str(result.get("quote_asset_id", "")).strip()
    if not base_asset_id or not quote_asset_id:
        raise ValueError("resolve_offer_asset_ids_missing_fields")
    return {"base_asset_id": base_asset_id, "quote_asset_id": quote_asset_id}


def program_config_path_from_payload(payload: dict[str, Any]) -> str | None:
    """Resolve program.yaml path from signing/offer payload fields."""
    for key in ("program_config_path", "program_config", "program_path"):
        value = str(payload.get(key, "")).strip()
        if value:
            return value
    home = str(payload.get("program_home_dir", "")).strip()
    if home:
        return str(Path(home).expanduser() / "config" / "program.yaml")
    return None


def is_vault_kms_payload(payload: dict[str, Any]) -> bool:
    """True when payload should use vault KMS signing (Rust signer)."""
    return bool(str(payload.get("signer_kms_key_id", "")).strip())


def expires_at_unix_from_payload(payload: dict[str, Any]) -> int | None:
    expiry_unit = str(payload.get("expiry_unit", "")).strip()
    try:
        expiry_value = int(payload.get("expiry_value", 0) or 0)
    except (TypeError, ValueError):
        expiry_value = 0
    if not expiry_unit or expiry_value <= 0:
        return None
    expires_at = datetime.datetime.now(datetime.UTC) + datetime.timedelta(
        **{expiry_unit: expiry_value}
    )
    return int(expires_at.timestamp())


def vault_offer_request_from_payload(
    payload: dict[str, Any], plan: dict[str, Any]
) -> dict[str, Any]:
    offer_asset_id = str(plan.get("offer_asset_id", payload.get("asset_id", ""))).strip().lower()
    request_asset_id = str(plan.get("request_asset_id", "")).strip().lower()
    raw_offer_coin_ids = plan.get("offer_coin_ids", [])
    offer_coin_ids = (
        [str(value).strip().lower() for value in raw_offer_coin_ids if str(value).strip()]
        if isinstance(raw_offer_coin_ids, list)
        else []
    )
    raw_presplit_coin_ids = payload.get("presplit_coin_ids", plan.get("presplit_coin_ids", []))
    presplit_coin_ids = (
        [str(value).strip().lower() for value in raw_presplit_coin_ids if str(value).strip()]
        if isinstance(raw_presplit_coin_ids, list)
        else []
    )
    request: dict[str, Any] = {
        "receive_address": str(payload.get("receive_address", "")).strip(),
        "offer_asset_id": offer_asset_id,
        "offer_amount": int(plan.get("offer_amount", 0)),
        "request_asset_id": request_asset_id,
        "request_amount": int(plan.get("request_amount", 0)),
        "offer_coin_ids": offer_coin_ids,
        "presplit_coin_ids": presplit_coin_ids,
        "split_input_coins": bool(payload.get("split_input_coins", True)),
        "broadcast_split": bool(payload.get("broadcast_split", False)),
    }
    expires_at = expires_at_unix_from_payload({**payload, **plan})
    if expires_at is not None:
        request["expires_at"] = expires_at
    return request


def vault_mixed_split_request_from_payload(payload: dict[str, Any]) -> dict[str, Any]:
    raw_outputs = payload.get("output_amounts_base_units", [])
    output_amounts: list[int] = []
    if isinstance(raw_outputs, list):
        for value in raw_outputs:
            output_amounts.append(int(value))
    raw_coin_ids = payload.get("selected_coin_ids", [])
    coin_ids: list[str] = []
    if isinstance(raw_coin_ids, list):
        for value in raw_coin_ids:
            clean = str(value).strip().lower()
            if clean.startswith("0x"):
                clean = clean[2:]
            if clean:
                coin_ids.append(clean)
    return {
        "receive_address": str(payload.get("receive_address", "")).strip(),
        "asset_id": str(payload.get("asset_id", "")).strip().lower(),
        "output_amounts": output_amounts,
        "coin_ids": coin_ids,
        "allow_sub_cat_output": bool(payload.get("allow_sub_cat_output", False)),
        "fee_mojos": int(payload.get("fee_mojos", 0)),
    }


def build_vault_offer_from_payload(payload: dict[str, Any]) -> tuple[str | None, str | None]:
    program_path = program_config_path_from_payload(payload)
    if not program_path:
        return None, "missing_program_config_path"
    plan = payload.get("plan") or {}
    if not isinstance(plan, dict):
        return None, "missing_plan"
    try:
        result = build_vault_cat_offer(
            program_path,
            vault_offer_request_from_payload(payload, plan),
        )
    except ImportError as exc:
        return None, str(exc)
    except Exception as exc:
        return None, f"rust_signer_offer_failed:{exc}"
    spend_bundle_hex = str(result.get("spend_bundle_hex", "")).strip()
    if not spend_bundle_hex:
        return None, "missing_spend_bundle_hex"
    return spend_bundle_hex, None


def sign_and_broadcast_vault_mixed_split(payload: dict[str, Any]) -> dict[str, Any]:
    program_path = program_config_path_from_payload(payload)
    if not program_path:
        return {
            "status": "skipped",
            "reason": "missing_program_config_path",
            "operation_id": None,
        }
    request = vault_mixed_split_request_from_payload(payload)
    request["broadcast"] = True
    try:
        result = build_mixed_split(program_path, request)
    except ImportError as exc:
        return {"status": "skipped", "reason": str(exc), "operation_id": None}
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"rust_signer_mixed_split_failed:{exc}",
            "operation_id": None,
        }
    spend_bundle_hex = str(result.get("spend_bundle_hex", "")).strip()
    if not spend_bundle_hex:
        return {
            "status": "skipped",
            "reason": "missing_spend_bundle_hex",
            "operation_id": None,
        }
    try:
        sdk = importlib.import_module("chia_wallet_sdk")
        raw_hex = (
            spend_bundle_hex[2:] if spend_bundle_hex.lower().startswith("0x") else spend_bundle_hex
        )
        spend_bundle = sdk.SpendBundle.from_bytes(bytes.fromhex(raw_hex))
        operation_id = sdk.to_hex(spend_bundle.hash())
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"spend_bundle_decode_error:{exc}",
            "operation_id": None,
        }
    broadcast_status = str(result.get("broadcast_status", "")).strip() or "submitted"
    return {
        "status": "executed",
        "reason": broadcast_status,
        "operation_id": operation_id,
    }

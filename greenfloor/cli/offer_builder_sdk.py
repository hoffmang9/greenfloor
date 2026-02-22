from __future__ import annotations

import json
import sys
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

    asset_id = str(payload.get("asset_id", "xch")).strip().lower() or "xch"
    if asset_id not in {"xch", "1"}:
        raise ValueError("asset_not_supported_yet")

    result = build_signed_spend_bundle(
        {
            "key_id": key_id,
            "network": network,
            "receive_address": receive_address,
            "keyring_yaml_path": keyring_yaml_path,
            "asset_id": "xch",
            "plan": {
                "op_type": "split",
                "size_base_units": size_base_units,
                "op_count": 1,
                "target_total_base_units": size_base_units,
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


def main() -> None:
    raw = sys.stdin.read()
    try:
        payload = json.loads(raw or "{}")
    except json.JSONDecodeError:
        print(json.dumps({"status": "skipped", "reason": "invalid_request_json"}))
        raise SystemExit(0) from None
    if not isinstance(payload, dict):
        print(json.dumps({"status": "skipped", "reason": "invalid_request_payload"}))
        raise SystemExit(0)

    try:
        sdk = _import_sdk()
    except Exception as exc:
        print(json.dumps({"status": "skipped", "reason": f"wallet_sdk_import_error:{exc}"}))
        raise SystemExit(0) from None

    try:
        offer = _build_offer(payload, sdk)
    except Exception as exc:
        print(
            json.dumps(
                {
                    "status": "skipped",
                    "reason": f"wallet_sdk_offer_build_failed:{exc}",
                }
            )
        )
        raise SystemExit(0) from None

    print(
        json.dumps(
            {
                "status": "executed",
                "reason": "wallet_sdk_offer_build_success",
                "offer": offer,
            }
        )
    )


if __name__ == "__main__":
    main()

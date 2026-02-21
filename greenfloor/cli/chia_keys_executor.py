from __future__ import annotations

import asyncio
import importlib
import json
import os
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Any


def execute_payload(payload: dict[str, Any]) -> dict[str, Any]:
    selected_source = str(payload.get("selected_source", "")).strip()
    if selected_source != "chia_keys":
        return {"status": "skipped", "reason": "unsupported_selected_source", "operation_id": None}

    signer_selection = payload.get("signer_selection") or {}
    if not isinstance(signer_selection, dict):
        return {"status": "skipped", "reason": "missing_signer_selection", "operation_id": None}
    keyring_yaml_path = str(signer_selection.get("keyring_yaml_path", "")).strip()
    if not keyring_yaml_path:
        return {"status": "skipped", "reason": "missing_keyring_yaml_path", "operation_id": None}
    if not Path(keyring_yaml_path).expanduser().exists():
        return {"status": "skipped", "reason": "keyring_yaml_not_found", "operation_id": None}

    plan = payload.get("plan") or {}
    if not isinstance(plan, dict):
        return {"status": "skipped", "reason": "missing_plan", "operation_id": None}
    op_type = str(plan.get("op_type", "")).strip()
    if op_type not in {"split", "combine"}:
        return {"status": "skipped", "reason": "unsupported_operation_type", "operation_id": None}

    # Built-in path currently supports only XCH split/combine until CAT spend plumbing is added.
    asset_id = str(payload.get("asset_id", "")).strip().lower()
    if asset_id not in {"xch", "1", ""}:
        return {"status": "skipped", "reason": "asset_not_supported_yet", "operation_id": None}

    backend_cmd = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", "").strip()
    if not backend_cmd:
        backend_cmd = f"{sys.executable} -m greenfloor.cli.chia_keys_signer_backend"
    if not backend_cmd:
        return {
            "status": "skipped",
            "reason": "chia_keys_signer_backend_not_configured",
            "operation_id": None,
        }
    backend_payload = dict(payload)
    backend_payload["keyring_yaml_path"] = keyring_yaml_path

    try:
        completed = subprocess.run(
            shlex.split(backend_cmd),
            input=json.dumps(backend_payload),
            capture_output=True,
            check=False,
            text=True,
            timeout=120,
        )
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"signer_backend_spawn_error:{exc}",
            "operation_id": None,
        }

    if completed.returncode != 0:
        err = completed.stderr.strip() or completed.stdout.strip() or "unknown_error"
        return {"status": "skipped", "reason": f"signer_backend_failed:{err}", "operation_id": None}

    try:
        body = json.loads(completed.stdout.strip() or "{}")
    except json.JSONDecodeError:
        return {"status": "skipped", "reason": "signer_backend_invalid_json", "operation_id": None}

    raw_spend_bundle_hex = body.get("spend_bundle_hex")
    spend_bundle_hex = str(raw_spend_bundle_hex).strip() if raw_spend_bundle_hex is not None else ""
    if spend_bundle_hex:
        broadcast = _broadcast_spend_bundle(
            spend_bundle_hex=spend_bundle_hex,
            network=str(payload.get("network", "")).strip(),
        )
        if broadcast["status"] == "executed":
            return broadcast
        return {
            "status": "skipped",
            "reason": f"broadcast_failed:{broadcast['reason']}",
            "operation_id": None,
        }

    return {
        "status": str(body.get("status", "executed")),
        "reason": str(body.get("reason", "chia_keys_signer_backend_success")),
        "operation_id": (
            str(body.get("operation_id")) if body.get("operation_id") is not None else None
        ),
    }


def _broadcast_spend_bundle(*, spend_bundle_hex: str, network: str) -> dict[str, Any]:
    try:
        sdk = importlib.import_module("chia_wallet_sdk")
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"wallet_sdk_import_error:{exc}",
            "operation_id": None,
        }

    try:
        spend_bundle_bytes = bytes.fromhex(
            spend_bundle_hex[2:] if spend_bundle_hex.lower().startswith("0x") else spend_bundle_hex
        )
    except ValueError:
        return {"status": "skipped", "reason": "invalid_spend_bundle_hex", "operation_id": None}

    try:
        spend_bundle = sdk.SpendBundle.from_bytes(spend_bundle_bytes)
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"spend_bundle_decode_error:{exc}",
            "operation_id": None,
        }

    async def _push() -> dict[str, Any]:
        try:
            custom_url = os.getenv("GREENFLOOR_WALLET_SDK_COINSET_URL", "").strip()
            if custom_url:
                client = sdk.RpcClient(custom_url)
            elif network == "testnet11":
                client = sdk.RpcClient.testnet11()
            else:
                client = sdk.RpcClient.mainnet()
            response = await client.push_tx(spend_bundle)
        except Exception as exc:
            return {"status": "skipped", "reason": f"push_tx_error:{exc}", "operation_id": None}
        if not getattr(response, "success", False):
            err = getattr(response, "error", None) or "push_tx_rejected"
            return {"status": "skipped", "reason": str(err), "operation_id": None}
        tx_id = sdk.to_hex(spend_bundle.hash())
        return {
            "status": "executed",
            "reason": str(getattr(response, "status", "submitted")),
            "operation_id": tx_id,
        }

    try:
        return asyncio.run(_push())
    except RuntimeError as exc:
        return {"status": "skipped", "reason": f"push_tx_runtime_error:{exc}", "operation_id": None}


def main() -> None:
    raw = sys.stdin.read().strip()
    if not raw:
        raise SystemExit(2)
    payload = json.loads(raw)
    if not isinstance(payload, dict):
        raise SystemExit(2)
    print(json.dumps(execute_payload(payload)))


if __name__ == "__main__":
    main()

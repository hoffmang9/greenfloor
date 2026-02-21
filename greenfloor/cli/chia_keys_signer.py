from __future__ import annotations

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

    key_id = str(payload.get("key_id", "")).strip()
    network = str(payload.get("network", "")).strip()
    keyring_yaml_path = str(payload.get("keyring_yaml_path", "")).strip()
    receive_address = str(payload.get("receive_address", "")).strip()
    if not key_id or not network:
        return {"status": "skipped", "reason": "missing_key_or_network", "operation_id": None}
    if not keyring_yaml_path:
        return {"status": "skipped", "reason": "missing_keyring_yaml_path", "operation_id": None}
    if not Path(keyring_yaml_path).expanduser().exists():
        return {"status": "skipped", "reason": "keyring_yaml_not_found", "operation_id": None}
    if not receive_address:
        return {"status": "skipped", "reason": "missing_receive_address", "operation_id": None}

    plan = payload.get("plan") or {}
    if not isinstance(plan, dict):
        return {"status": "skipped", "reason": "missing_plan", "operation_id": None}
    op_type = str(plan.get("op_type", "")).strip()
    size_base_units = int(plan.get("size_base_units", 0))
    op_count = int(plan.get("op_count", 0))
    target_total_base_units = int(plan.get("target_total_base_units", size_base_units * op_count))
    if op_type not in {"split", "combine"}:
        return {"status": "skipped", "reason": "unsupported_operation_type", "operation_id": None}
    if size_base_units <= 0 or op_count <= 0 or target_total_base_units <= 0:
        return {"status": "skipped", "reason": "invalid_plan_values", "operation_id": None}

    asset_id = str(payload.get("asset_id", "")).strip().lower()
    if asset_id not in {"xch", "1", ""}:
        return {"status": "skipped", "reason": "asset_not_supported_yet", "operation_id": None}

    backend_request = {
        "key_id": key_id,
        "network": network,
        "keyring_yaml_path": keyring_yaml_path,
        "receive_address": receive_address,
        "asset_id": asset_id,
        "market_id": str(payload.get("market_id", "")).strip(),
        "plan": {
            "op_type": op_type,
            "size_base_units": size_base_units,
            "op_count": op_count,
            "target_total_base_units": target_total_base_units,
            "reason": str(plan.get("reason", "")).strip(),
        },
    }

    backend_cmd = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", "").strip()
    if not backend_cmd:
        backend_cmd = f"{sys.executable} -m greenfloor.cli.chia_keys_signer_backend"

    try:
        completed = subprocess.run(
            shlex.split(backend_cmd),
            input=json.dumps(backend_request),
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

    spend_bundle_hex = body.get("spend_bundle_hex")
    status = str(body.get("status", "executed"))
    reason = str(body.get("reason", "chia_keys_signer_backend_success"))
    if spend_bundle_hex is None and status == "skipped":
        result = {
            "status": "skipped",
            "reason": reason,
            "operation_id": (
                str(body.get("operation_id")) if body.get("operation_id") is not None else None
            ),
        }
        if body.get("backend_request") is not None:
            result["backend_request"] = body.get("backend_request")
        if body.get("builder_request") is not None:
            result["builder_request"] = body.get("builder_request")
        return result
    if spend_bundle_hex is None:
        return {
            "status": "skipped",
            "reason": "signer_backend_missing_spend_bundle_hex",
            "operation_id": None,
        }

    return {
        "status": status,
        "reason": reason,
        "operation_id": (
            str(body.get("operation_id")) if body.get("operation_id") is not None else None
        ),
        "spend_bundle_hex": str(spend_bundle_hex).strip(),
    }


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

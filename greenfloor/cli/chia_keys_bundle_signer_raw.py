from __future__ import annotations

import json
import os
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Any


def execute_payload(payload: dict[str, Any]) -> dict[str, Any]:
    key_id = str(payload.get("key_id", "")).strip()
    network = str(payload.get("network", "")).strip()
    receive_address = str(payload.get("receive_address", "")).strip()
    keyring_yaml_path = str(payload.get("keyring_yaml_path", "")).strip()
    asset_id = str(payload.get("asset_id", "")).strip().lower()
    if not key_id or not network or not receive_address:
        return {
            "status": "skipped",
            "reason": "missing_key_or_network_or_address",
            "operation_id": None,
        }
    if not keyring_yaml_path:
        return {"status": "skipped", "reason": "missing_keyring_yaml_path", "operation_id": None}
    if not Path(keyring_yaml_path).expanduser().exists():
        return {"status": "skipped", "reason": "keyring_yaml_not_found", "operation_id": None}
    if asset_id not in {"xch", "1", ""}:
        return {"status": "skipped", "reason": "asset_not_supported_yet", "operation_id": None}

    plan = payload.get("plan") or {}
    selected_coins = payload.get("selected_coins") or []
    if not isinstance(plan, dict):
        return {"status": "skipped", "reason": "missing_plan", "operation_id": None}
    if not isinstance(selected_coins, list) or not selected_coins:
        return {"status": "skipped", "reason": "missing_selected_coins", "operation_id": None}
    op_type = str(plan.get("op_type", "")).strip()
    if op_type not in {"split", "combine"}:
        return {"status": "skipped", "reason": "unsupported_operation_type", "operation_id": None}

    engine_request = {
        "key_id": key_id,
        "network": network,
        "receive_address": receive_address,
        "keyring_yaml_path": keyring_yaml_path,
        "asset_id": asset_id,
        "plan": plan,
        "selected_coins": selected_coins,
    }
    engine_cmd = os.getenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_CMD", "").strip()
    if not engine_cmd:
        engine_cmd = f"{sys.executable} -m greenfloor.cli.chia_keys_raw_engine"

    try:
        completed = subprocess.run(
            shlex.split(engine_cmd),
            input=json.dumps(engine_request),
            capture_output=True,
            check=False,
            text=True,
            timeout=120,
        )
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"raw_engine_spawn_error:{exc}",
            "operation_id": None,
        }
    if completed.returncode != 0:
        err = completed.stderr.strip() or completed.stdout.strip() or "unknown_error"
        return {"status": "skipped", "reason": f"raw_engine_failed:{err}", "operation_id": None}
    try:
        body = json.loads(completed.stdout.strip() or "{}")
    except json.JSONDecodeError:
        return {"status": "skipped", "reason": "raw_engine_invalid_json", "operation_id": None}

    spend_bundle_hex = body.get("spend_bundle_hex")
    status = str(body.get("status", "executed"))
    reason = str(body.get("reason", "chia_keys_raw_sign_success"))
    if spend_bundle_hex is None and status == "skipped":
        result = {
            "status": "skipped",
            "reason": reason,
            "operation_id": (
                str(body.get("operation_id")) if body.get("operation_id") is not None else None
            ),
            "engine_request": engine_request,
        }
        if body.get("sign_job") is not None:
            result["sign_job"] = body.get("sign_job")
        return result
    if spend_bundle_hex is None:
        return {
            "status": "skipped",
            "reason": "raw_engine_missing_spend_bundle_hex",
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

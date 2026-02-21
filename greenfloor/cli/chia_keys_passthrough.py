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

    signer = payload.get("signer_selection") or {}
    if not isinstance(signer, dict):
        return {"status": "skipped", "reason": "missing_signer_selection", "operation_id": None}
    key_id = str(payload.get("key_id", "")).strip()
    signer_key_id = str(signer.get("key_id", "")).strip()
    if not key_id or not signer_key_id or key_id != signer_key_id:
        return {"status": "skipped", "reason": "signer_key_mismatch", "operation_id": None}
    keyring_yaml_path = str(signer.get("keyring_yaml_path", "")).strip()
    if not keyring_yaml_path:
        return {"status": "skipped", "reason": "missing_keyring_yaml_path", "operation_id": None}
    if not Path(keyring_yaml_path).expanduser().exists():
        return {"status": "skipped", "reason": "keyring_yaml_not_found", "operation_id": None}

    receive_address = str(payload.get("receive_address", "")).strip()
    if not receive_address:
        return {"status": "skipped", "reason": "missing_receive_address", "operation_id": None}

    plan = payload.get("plan") or {}
    if not isinstance(plan, dict):
        return {"status": "skipped", "reason": "missing_plan", "operation_id": None}
    op_type = str(plan.get("op_type", "")).strip()
    if op_type not in {"split", "combine"}:
        return {"status": "skipped", "reason": "unsupported_operation_type", "operation_id": None}
    size_base_units = int(plan.get("size_base_units", 0))
    op_count = int(plan.get("op_count", 0))
    if size_base_units <= 0 or op_count <= 0:
        return {"status": "skipped", "reason": "invalid_plan_values", "operation_id": None}

    asset_id = str(payload.get("asset_id", "")).strip().lower()
    if asset_id not in {"xch", "1", ""}:
        return {"status": "skipped", "reason": "asset_not_supported_yet", "operation_id": None}

    worker_payload = {
        "selected_source": selected_source,
        "key_id": key_id,
        "network": str(payload.get("network", "")).strip(),
        "market_id": str(payload.get("market_id", "")).strip(),
        "asset_id": asset_id,
        "receive_address": receive_address,
        "keyring_yaml_path": keyring_yaml_path,
        "chia_keys_dir": str(signer.get("chia_keys_dir", "")).strip(),
        "plan": {
            "op_type": op_type,
            "size_base_units": size_base_units,
            "op_count": op_count,
            "reason": str(plan.get("reason", "")).strip(),
            "target_total_base_units": size_base_units * op_count,
        },
    }

    worker_cmd = os.getenv("GREENFLOOR_CHIA_KEYS_PASSTHROUGH_WORKER_CMD", "").strip()
    if not worker_cmd:
        worker_cmd = f"{sys.executable} -m greenfloor.cli.chia_keys_worker"

    try:
        completed = subprocess.run(
            shlex.split(worker_cmd),
            input=json.dumps(worker_payload),
            capture_output=True,
            check=False,
            text=True,
            timeout=120,
        )
    except Exception as exc:
        return {"status": "skipped", "reason": f"worker_spawn_error:{exc}", "operation_id": None}

    if completed.returncode != 0:
        err = completed.stderr.strip() or completed.stdout.strip() or "unknown_error"
        return {"status": "skipped", "reason": f"worker_failed:{err}", "operation_id": None}

    try:
        body = json.loads(completed.stdout.strip() or "{}")
    except json.JSONDecodeError:
        return {"status": "skipped", "reason": "worker_invalid_json", "operation_id": None}

    return {
        "status": str(body.get("status", "executed")),
        "reason": str(body.get("reason", "chia_keys_worker_success")),
        "operation_id": (
            str(body.get("operation_id")) if body.get("operation_id") is not None else None
        ),
        "spend_bundle_hex": (
            str(body.get("spend_bundle_hex")).strip()
            if body.get("spend_bundle_hex") is not None
            else None
        ),
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

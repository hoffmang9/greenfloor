from __future__ import annotations

import json
import os
import shlex
import subprocess
import sys
from typing import Any


def execute_payload(payload: dict[str, Any]) -> dict[str, Any]:
    selected_source = str(payload.get("selected_source", "")).strip()
    if not selected_source:
        return {"status": "skipped", "reason": "missing_selected_source", "operation_id": None}

    key_id = str(payload.get("key_id", "")).strip()
    network = str(payload.get("network", "")).strip()
    if not key_id or not network:
        return {"status": "skipped", "reason": "missing_key_or_network", "operation_id": None}

    source_env_map = {
        "chia_keys": "GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD",
        "mnemonic_import": "GREENFLOOR_MNEMONIC_EXECUTOR_CMD",
        "generate_new_key": "GREENFLOOR_GENERATE_KEY_EXECUTOR_CMD",
    }
    cmd_env = source_env_map.get(selected_source)
    if cmd_env is None:
        return {"status": "skipped", "reason": "unsupported_selected_source", "operation_id": None}
    cmd_raw = os.getenv(cmd_env, "").strip()
    if not cmd_raw and selected_source == "chia_keys":
        cmd_raw = f"{sys.executable} -m greenfloor.cli.chia_keys_executor"
    if not cmd_raw:
        return {
            "status": "skipped",
            "reason": f"{selected_source}_executor_not_configured",
            "operation_id": None,
        }

    try:
        completed = subprocess.run(
            shlex.split(cmd_raw),
            input=json.dumps(payload),
            capture_output=True,
            check=False,
            text=True,
            timeout=120,
        )
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"delegate_spawn_error:{exc}",
            "operation_id": None,
        }
    if completed.returncode != 0:
        err = completed.stderr.strip() or completed.stdout.strip() or "unknown_error"
        return {
            "status": "skipped",
            "reason": f"delegate_failed:{err}",
            "operation_id": None,
        }
    try:
        body = json.loads(completed.stdout.strip() or "{}")
    except json.JSONDecodeError:
        return {"status": "skipped", "reason": "delegate_invalid_json", "operation_id": None}
    return {
        "status": str(body.get("status", "executed")),
        "reason": str(body.get("reason", "delegate_success")),
        "operation_id": (
            str(body.get("operation_id")) if body.get("operation_id") is not None else None
        ),
    }


def main() -> None:
    raw = sys.stdin.read().strip()
    if not raw:
        raise SystemExit(2)
    payload = json.loads(raw)
    if not isinstance(payload, dict):
        raise SystemExit(2)
    result = execute_payload(payload)
    print(json.dumps(result))


if __name__ == "__main__":
    main()

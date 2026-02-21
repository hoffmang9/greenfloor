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
    size_base_units = int(plan.get("size_base_units", 0))
    op_count = int(plan.get("op_count", 0))
    target_total = int(plan.get("target_total_base_units", 0))
    if op_type not in {"split", "combine"}:
        return {"status": "skipped", "reason": "unsupported_operation_type", "operation_id": None}
    if size_base_units <= 0 or op_count <= 0 or target_total <= 0:
        return {"status": "skipped", "reason": "invalid_plan_values", "operation_id": None}

    selected_total = 0
    additions: list[dict[str, Any]] = []
    for coin in selected_coins:
        if not isinstance(coin, dict):
            continue
        try:
            amount = int(coin.get("amount", 0))
        except (TypeError, ValueError):
            continue
        selected_total += amount
    if selected_total < target_total:
        return {
            "status": "skipped",
            "reason": "insufficient_selected_coin_total",
            "operation_id": None,
        }

    # For v1 split/combine tx construction, we produce repeated same-address outputs.
    # UTXO and fee behavior remains controlled by the downstream signer command.
    for _ in range(op_count):
        additions.append({"address": receive_address, "amount": size_base_units})
    change = selected_total - target_total
    if change > 0:
        additions.append({"address": receive_address, "amount": change})

    sign_tx_request = {
        "key_id": key_id,
        "network": network,
        "receive_address": receive_address,
        "keyring_yaml_path": keyring_yaml_path,
        "asset_id": asset_id,
        "plan": plan,
        "selected_coins": selected_coins,
        "selected_total_base_units": selected_total,
        "additions": additions,
    }

    submit_cmd = os.getenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD", "").strip()
    if not submit_cmd:
        submit_cmd = f"{sys.executable} -m greenfloor.cli.chia_keys_raw_engine_sign_impl_sdk_submit"

    try:
        completed = subprocess.run(
            shlex.split(submit_cmd),
            input=json.dumps(sign_tx_request),
            capture_output=True,
            check=False,
            text=True,
            timeout=120,
        )
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"sdk_submit_spawn_error:{exc}",
            "operation_id": None,
        }
    if completed.returncode != 0:
        err = completed.stderr.strip() or completed.stdout.strip() or "unknown_error"
        return {"status": "skipped", "reason": f"sdk_submit_failed:{err}", "operation_id": None}
    try:
        body = json.loads(completed.stdout.strip() or "{}")
    except json.JSONDecodeError:
        return {"status": "skipped", "reason": "sdk_submit_invalid_json", "operation_id": None}

    spend_bundle_hex = body.get("spend_bundle_hex")
    status = str(body.get("status", "executed"))
    reason = str(body.get("reason", "sdk_submit_success"))
    if spend_bundle_hex is None and status == "skipped":
        result = {
            "status": "skipped",
            "reason": reason,
            "operation_id": (
                str(body.get("operation_id")) if body.get("operation_id") is not None else None
            ),
            "sign_tx_request": sign_tx_request,
        }
        return result
    if spend_bundle_hex is None:
        return {
            "status": "skipped",
            "reason": "sdk_submit_missing_spend_bundle_hex",
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

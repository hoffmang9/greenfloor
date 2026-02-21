from __future__ import annotations

import asyncio
import importlib
import json
import os
import shlex
import subprocess
import sys
from typing import Any


def _list_unspent_xch_coins(
    *,
    sdk: Any,
    receive_address: str,
    network: str,
) -> list[Any]:
    async def _fetch() -> list[Any]:
        address = sdk.Address.decode(receive_address)
        puzzle_hash = address.puzzle_hash
        custom_url = os.getenv("GREENFLOOR_WALLET_SDK_COINSET_URL", "").strip()
        if custom_url:
            client = sdk.RpcClient(custom_url)
        elif network == "testnet11":
            client = sdk.RpcClient.testnet11()
        else:
            client = sdk.RpcClient.mainnet()
        response = await client.get_coin_records_by_puzzle_hash(
            puzzle_hash, includeSpentCoins=False
        )
        if not getattr(response, "success", False):
            return []
        records = getattr(response, "coin_records", None) or []
        return [r.coin for r in records if getattr(r, "coin", None) is not None]

    try:
        return asyncio.run(_fetch())
    except Exception:
        return []


def _build_additions_from_plan(
    *,
    plan: dict[str, Any],
    selected_coins: list[dict[str, Any]],
    receive_address: str,
) -> tuple[list[dict[str, Any]] | None, str | None]:
    op_type = str(plan.get("op_type", "")).strip()
    size_base_units = int(plan.get("size_base_units", 0))
    op_count = int(plan.get("op_count", 0))
    target_total = int(plan.get("target_total_base_units", 0))
    if target_total <= 0 and size_base_units > 0 and op_count > 0:
        target_total = size_base_units * op_count
    if op_type not in {"split", "combine"}:
        return None, "unsupported_operation_type"
    if size_base_units <= 0 or op_count <= 0 or target_total <= 0:
        return None, "invalid_plan_values"

    selected_total = 0
    for coin in selected_coins:
        try:
            selected_total += int(coin.get("amount", 0))
        except (TypeError, ValueError):
            continue
    if selected_total < target_total:
        return None, "insufficient_selected_coin_total"

    additions: list[dict[str, Any]] = []
    for _ in range(op_count):
        additions.append({"address": receive_address, "amount": size_base_units})
    change = selected_total - target_total
    if change > 0:
        additions.append({"address": receive_address, "amount": change})
    return additions, None


def execute_payload(payload: dict[str, Any]) -> dict[str, Any]:
    key_id = str(payload.get("key_id", "")).strip()
    network = str(payload.get("network", "")).strip()
    receive_address = str(payload.get("receive_address", "")).strip()
    asset_id = str(payload.get("asset_id", "")).strip().lower()
    keyring_yaml_path = str(payload.get("keyring_yaml_path", "")).strip()
    if not key_id or not network or not receive_address:
        return {
            "status": "skipped",
            "reason": "missing_key_or_network_or_address",
            "operation_id": None,
        }
    if not keyring_yaml_path:
        return {"status": "skipped", "reason": "missing_keyring_yaml_path", "operation_id": None}
    if asset_id not in {"xch", "1", ""}:
        return {"status": "skipped", "reason": "asset_not_supported_yet", "operation_id": None}

    plan = payload.get("plan") or {}
    if not isinstance(plan, dict):
        return {"status": "skipped", "reason": "missing_plan", "operation_id": None}
    op_type = str(plan.get("op_type", "")).strip()
    size_base_units = int(plan.get("size_base_units", 0))
    op_count = int(plan.get("op_count", 0))
    target_total = int(plan.get("target_total_base_units", 0))
    if target_total <= 0 and size_base_units > 0 and op_count > 0:
        target_total = size_base_units * op_count
        plan = dict(plan)
        plan["target_total_base_units"] = target_total
    if op_type not in {"split", "combine"} or target_total <= 0:
        return {"status": "skipped", "reason": "invalid_plan", "operation_id": None}

    sign_cmd = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", "").strip()
    if not sign_cmd:
        sign_cmd = f"{sys.executable} -m greenfloor.cli.chia_keys_raw_engine_sign_impl_sdk_submit"

    try:
        sdk = importlib.import_module("chia_wallet_sdk")
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"wallet_sdk_import_error:{exc}",
            "operation_id": None,
        }

    coins = _list_unspent_xch_coins(sdk=sdk, receive_address=receive_address, network=network)
    if not coins:
        return {"status": "skipped", "reason": "no_unspent_xch_coins", "operation_id": None}
    try:
        selected = sdk.select_coins(coins, target_total)
    except Exception as exc:
        return {"status": "skipped", "reason": f"coin_selection_failed:{exc}", "operation_id": None}

    sign_request_selected_coins = [
        {
            "coin_id": sdk.to_hex(c.coin_id()),
            "parent_coin_info": sdk.to_hex(c.parent_coin_info),
            "puzzle_hash": sdk.to_hex(c.puzzle_hash),
            "amount": int(c.amount),
        }
        for c in selected
    ]
    additions, additions_error = _build_additions_from_plan(
        plan=plan,
        selected_coins=sign_request_selected_coins,
        receive_address=receive_address,
    )
    if additions_error is not None:
        return {"status": "skipped", "reason": additions_error, "operation_id": None}

    sign_request = {
        "key_id": key_id,
        "network": network,
        "receive_address": receive_address,
        "keyring_yaml_path": keyring_yaml_path,
        "asset_id": asset_id,
        "plan": plan,
        "selected_coins": sign_request_selected_coins,
        "additions": additions,
    }

    try:
        completed = subprocess.run(
            shlex.split(sign_cmd),
            input=json.dumps(sign_request),
            capture_output=True,
            check=False,
            text=True,
            timeout=120,
        )
    except Exception as exc:
        return {"status": "skipped", "reason": f"sign_spawn_error:{exc}", "operation_id": None}
    if completed.returncode != 0:
        err = completed.stderr.strip() or completed.stdout.strip() or "unknown_error"
        return {"status": "skipped", "reason": f"sign_failed:{err}", "operation_id": None}
    try:
        body = json.loads(completed.stdout.strip() or "{}")
    except json.JSONDecodeError:
        return {"status": "skipped", "reason": "sign_invalid_json", "operation_id": None}
    spend_bundle_hex = body.get("spend_bundle_hex")
    if spend_bundle_hex is None:
        return {
            "status": "skipped",
            "reason": "sign_missing_spend_bundle_hex",
            "operation_id": None,
        }
    return {
        "status": str(body.get("status", "executed")),
        "reason": str(body.get("reason", "backend_sign_success")),
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

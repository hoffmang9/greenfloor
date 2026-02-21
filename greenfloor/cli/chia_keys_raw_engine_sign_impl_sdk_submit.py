from __future__ import annotations

import importlib
import json
import os
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Any

_AGG_SIG_ADDITIONAL_DATA_BY_NETWORK: dict[str, bytes] = {
    "mainnet": bytes.fromhex("37a90eb5185a9c4439a91ddc98bbadce7b4feba060d50116a067de66bf236615"),
    "testnet11": bytes.fromhex("b0a306abe27407130586c8e13d06dc057d4538c201dbd36c8f8c481f5e51af5c"),
}


def _hex_to_bytes(value: str) -> bytes:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if len(raw) % 2:
        raw = f"0{raw}"
    return bytes.fromhex(raw)


def _parse_fingerprint(key_id: str) -> int | None:
    raw = key_id.strip()
    if raw.isdigit():
        return int(raw)
    if raw.startswith("fingerprint:") and raw.removeprefix("fingerprint:").isdigit():
        return int(raw.removeprefix("fingerprint:"))
    mapping_raw = os.getenv("GREENFLOOR_KEY_ID_FINGERPRINT_MAP_JSON", "").strip()
    if mapping_raw:
        try:
            mapping = json.loads(mapping_raw)
        except json.JSONDecodeError:
            return None
        if isinstance(mapping, dict):
            mapped = mapping.get(raw)
            if isinstance(mapped, int):
                return mapped
            if isinstance(mapped, str) and mapped.isdigit():
                return int(mapped)
    return None


def _load_master_private_key(keyring_yaml_path: str, key_id: str) -> tuple[Any | None, str | None]:
    fingerprint = _parse_fingerprint(key_id)
    if fingerprint is None:
        return None, "key_id_fingerprint_mapping_missing"
    try:
        chia_keychain = importlib.import_module("chia.util.keychain")
        chia_keyring_wrapper = importlib.import_module("chia.util.keyring_wrapper")
    except Exception as exc:
        return None, f"chia_keychain_import_error:{exc}"
    try:
        keys_root = Path(keyring_yaml_path).expanduser().resolve().parent
        chia_keyring_wrapper.KeyringWrapper.cleanup_shared_instance()
        chia_keychain.set_keys_root_path(keys_root)
        keychain = chia_keychain.Keychain()
        key_data = keychain.get_key(fingerprint, include_secrets=True)
    except Exception as exc:
        return None, f"key_lookup_error:{exc}"
    try:
        return key_data.private_key, None
    except Exception:
        return None, "key_secrets_unavailable"


def _build_spend_bundle(
    payload: dict[str, Any], submit_request: dict[str, Any]
) -> tuple[str | None, str | None]:
    master_private_key, key_error = _load_master_private_key(
        keyring_yaml_path=submit_request["keyring_yaml_path"],
        key_id=submit_request["key_id"],
    )
    if key_error:
        return None, key_error
    if master_private_key is None:
        return None, "key_secrets_unavailable"
    try:
        sdk = importlib.import_module("chia_wallet_sdk")
    except Exception as exc:
        return None, f"wallet_sdk_import_error:{exc}"
    additional_data = _AGG_SIG_ADDITIONAL_DATA_BY_NETWORK.get(submit_request["network"])
    if additional_data is None:
        return None, "unsupported_network_for_signing"
    try:
        conditions = importlib.import_module("chia.consensus.condition_tools")
        default_constants = importlib.import_module("chia.consensus.default_constants")
        serialized_program = importlib.import_module(
            "chia.types.blockchain_format.serialized_program"
        )
        chia_rs = importlib.import_module("chia_rs")
    except Exception as exc:
        return None, f"chia_signing_import_error:{exc}"

    try:
        master_sk = sdk.SecretKey.from_bytes(bytes(master_private_key))
    except Exception as exc:
        return None, f"master_key_conversion_error:{exc}"
    derivation_scan_limit = int(os.getenv("GREENFLOOR_CHIA_KEYS_DERIVATION_SCAN_LIMIT", "200"))
    selected_coin_puzzle_hashes = set()
    for item in payload.get("selected_coins", []):
        try:
            selected_coin_puzzle_hashes.add(_hex_to_bytes(str(item["puzzle_hash"])))
        except Exception as exc:
            return None, f"invalid_selected_coin_puzzle_hash:{exc}"
    synthetic_sk_by_puzzle_hash: dict[bytes, Any] = {}
    for index in range(derivation_scan_limit):
        for derive_fn in (master_sk.derive_unhardened_path, master_sk.derive_hardened_path):
            try:
                child_sk = derive_fn([12381, 8444, 2, index])
                synthetic_sk = child_sk.derive_synthetic()
                puzzle_hash = sdk.standard_puzzle_hash(synthetic_sk.public_key())
            except Exception:
                continue
            if (
                puzzle_hash in selected_coin_puzzle_hashes
                and puzzle_hash not in synthetic_sk_by_puzzle_hash
            ):
                synthetic_sk_by_puzzle_hash[puzzle_hash] = synthetic_sk
        if len(synthetic_sk_by_puzzle_hash) == len(selected_coin_puzzle_hashes):
            break
    if len(synthetic_sk_by_puzzle_hash) != len(selected_coin_puzzle_hashes):
        return None, "derivation_scan_failed_for_selected_coin"

    try:
        clvm = sdk.Clvm()
        change_puzzle_hash = sdk.Address.decode(submit_request["receive_address"]).puzzle_hash
        spends = sdk.Spends(clvm, change_puzzle_hash)
        for coin_data in payload.get("selected_coins", []):
            spends.add_xch(
                sdk.Coin(
                    _hex_to_bytes(str(coin_data["parent_coin_info"])),
                    _hex_to_bytes(str(coin_data["puzzle_hash"])),
                    int(coin_data["amount"]),
                )
            )
        actions: list[Any] = []
        for addition in payload.get("additions", []):
            actions.append(
                sdk.Action.send(
                    sdk.Id.xch(),
                    sdk.Address.decode(str(addition["address"])).puzzle_hash,
                    int(addition["amount"]),
                )
            )
        deltas = spends.apply(actions)
        finished = spends.prepare(deltas)
        for pending_spend in finished.pending_spends():
            coin = pending_spend.coin()
            synthetic_sk = synthetic_sk_by_puzzle_hash.get(coin.puzzle_hash)
            if synthetic_sk is None:
                return None, "missing_signing_key_for_pending_spend"
            delegated = clvm.delegated_spend(pending_spend.conditions())
            clvm.spend_standard_coin(coin, synthetic_sk.public_key(), delegated)
        coin_spends = clvm.coin_spends()
    except Exception as exc:
        return None, f"build_spend_bundle_error:{exc}"

    try:
        signatures = []
        sk_by_pk_bytes: dict[bytes, Any] = {}
        for synthetic_sk in synthetic_sk_by_puzzle_hash.values():
            sk_by_pk_bytes[synthetic_sk.public_key().to_bytes()] = synthetic_sk
        for coin_spend in coin_spends:
            puzzle_reveal = serialized_program.SerializedProgram.from_bytes(
                coin_spend.puzzle_reveal
            )
            solution = serialized_program.SerializedProgram.from_bytes(coin_spend.solution)
            conditions_dict = conditions.conditions_dict_for_solution(
                puzzle_reveal,
                solution,
                default_constants.DEFAULT_CONSTANTS.MAX_BLOCK_COST_CLVM,
            )
            coin_for_pkm = chia_rs.Coin(
                coin_spend.coin.parent_coin_info,
                coin_spend.coin.puzzle_hash,
                coin_spend.coin.amount,
            )
            for public_key, message in conditions.pkm_pairs_for_conditions_dict(
                conditions_dict,
                coin_for_pkm,
                additional_data,
            ):
                sk = sk_by_pk_bytes.get(bytes(public_key))
                if sk is None:
                    return None, "missing_private_key_for_agg_sig_target"
                sk_bytes = sk.to_bytes()
                synthetic_sk = chia_rs.PrivateKey.from_bytes(sk_bytes)
                signatures.append(chia_rs.AugSchemeMPL.sign(synthetic_sk, message))
        if not signatures:
            return None, "no_agg_sig_targets_found"
        aggregate_sig = chia_rs.AugSchemeMPL.aggregate(signatures)
        sdk_signature = sdk.Signature.from_bytes(bytes(aggregate_sig))
        spend_bundle = sdk.SpendBundle(coin_spends, sdk_signature)
        return sdk.to_hex(spend_bundle.to_bytes()), None
    except Exception as exc:
        return None, f"sign_spend_bundle_error:{exc}"


def execute_payload(payload: dict[str, Any]) -> dict[str, Any]:
    key_id = str(payload.get("key_id", "")).strip()
    network = str(payload.get("network", "")).strip()
    receive_address = str(payload.get("receive_address", "")).strip()
    keyring_yaml_path = str(payload.get("keyring_yaml_path", "")).strip()
    asset_id = str(payload.get("asset_id", "")).strip().lower()
    additions = payload.get("additions") or []
    selected_coins = payload.get("selected_coins") or []
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
    if not isinstance(additions, list) or not additions:
        return {"status": "skipped", "reason": "missing_additions", "operation_id": None}
    if not isinstance(selected_coins, list) or not selected_coins:
        return {"status": "skipped", "reason": "missing_selected_coins", "operation_id": None}

    submit_request = {
        "key_id": key_id,
        "network": network,
        "receive_address": receive_address,
        "keyring_yaml_path": keyring_yaml_path,
        "asset_id": asset_id,
        "plan": payload.get("plan"),
        "selected_coins": selected_coins,
        "additions": additions,
    }
    submit_cmd = os.getenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD", "").strip()
    if not submit_cmd:
        spend_bundle_hex, error = _build_spend_bundle(payload, submit_request)
        if spend_bundle_hex is None:
            return {
                "status": "skipped",
                "reason": f"sdk_submit_in_process_failed:{error}",
                "operation_id": None,
                "submit_request": submit_request,
            }
        return {
            "status": "executed",
            "reason": "sdk_submit_in_process_success",
            "operation_id": None,
            "spend_bundle_hex": spend_bundle_hex,
        }

    try:
        completed = subprocess.run(
            shlex.split(submit_cmd),
            input=json.dumps(submit_request),
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
    if spend_bundle_hex is None:
        return {
            "status": "skipped",
            "reason": "sdk_submit_missing_spend_bundle_hex",
            "operation_id": None,
        }
    return {
        "status": str(body.get("status", "executed")),
        "reason": str(body.get("reason", "sdk_submit_success")),
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

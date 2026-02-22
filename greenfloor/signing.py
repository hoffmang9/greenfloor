"""Unified signing module for GreenFloor.

Handles coin discovery, selection, spend-bundle construction, signing,
and optional broadcast. Used by both the daemon (coin-op execution via
WalletAdapter) and the manager (offer building via offer_builder_sdk).
"""

from __future__ import annotations

import asyncio
import importlib
import json
import os
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


def _import_sdk() -> Any:
    return importlib.import_module("chia_wallet_sdk")


# ---------------------------------------------------------------------------
# Coin discovery
# ---------------------------------------------------------------------------


def _list_unspent_xch_coins(*, sdk: Any, receive_address: str, network: str) -> list[Any]:
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


# ---------------------------------------------------------------------------
# Additions planning
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# Key loading
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# Spend-bundle construction & signing
# ---------------------------------------------------------------------------


def _build_spend_bundle(
    *,
    sdk: Any,
    payload: dict[str, Any],
    keyring_yaml_path: str,
    key_id: str,
    network: str,
    receive_address: str,
) -> tuple[str | None, str | None]:
    """Build and sign a spend bundle. Returns (hex, None) or (None, error)."""
    master_private_key, key_error = _load_master_private_key(keyring_yaml_path, key_id)
    if key_error:
        return None, key_error
    if master_private_key is None:
        return None, "key_secrets_unavailable"

    additional_data = _AGG_SIG_ADDITIONAL_DATA_BY_NETWORK.get(network)
    if additional_data is None:
        return None, "unsupported_network_for_signing"

    try:
        conditions_mod = importlib.import_module("chia.consensus.condition_tools")
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
    selected_coin_puzzle_hashes: set[bytes] = set()
    for item in payload.get("selected_coins", []):
        try:
            selected_coin_puzzle_hashes.add(_hex_to_bytes(str(item["puzzle_hash"])))
        except Exception as exc:
            return None, f"invalid_selected_coin_puzzle_hash:{exc}"

    synthetic_sk_by_puzzle_hash: dict[bytes, Any] = {}
    for index in range(derivation_scan_limit):
        for derive_fn in (
            master_sk.derive_unhardened_path,
            master_sk.derive_hardened_path,
        ):
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
        change_puzzle_hash = sdk.Address.decode(receive_address).puzzle_hash
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
            conditions_dict = conditions_mod.conditions_dict_for_solution(
                puzzle_reveal,
                solution,
                default_constants.DEFAULT_CONSTANTS.MAX_BLOCK_COST_CLVM,
            )
            coin_for_pkm = chia_rs.Coin(
                coin_spend.coin.parent_coin_info,
                coin_spend.coin.puzzle_hash,
                coin_spend.coin.amount,
            )
            for public_key, message in conditions_mod.pkm_pairs_for_conditions_dict(
                conditions_dict,
                coin_for_pkm,
                additional_data,
            ):
                sk = sk_by_pk_bytes.get(bytes(public_key))
                if sk is None:
                    return None, "missing_private_key_for_agg_sig_target"
                sk_bytes = sk.to_bytes()
                synthetic_sk_chia = chia_rs.PrivateKey.from_bytes(sk_bytes)
                signatures.append(chia_rs.AugSchemeMPL.sign(synthetic_sk_chia, message))
        if not signatures:
            return None, "no_agg_sig_targets_found"
        aggregate_sig = chia_rs.AugSchemeMPL.aggregate(signatures)
        sdk_signature = sdk.Signature.from_bytes(bytes(aggregate_sig))
        spend_bundle = sdk.SpendBundle(coin_spends, sdk_signature)
        return sdk.to_hex(spend_bundle.to_bytes()), None
    except Exception as exc:
        return None, f"sign_spend_bundle_error:{exc}"


# ---------------------------------------------------------------------------
# Broadcast
# ---------------------------------------------------------------------------


def _broadcast_spend_bundle(*, sdk: Any, spend_bundle_hex: str, network: str) -> dict[str, Any]:
    try:
        raw_hex = (
            spend_bundle_hex[2:] if spend_bundle_hex.lower().startswith("0x") else spend_bundle_hex
        )
        spend_bundle_bytes = bytes.fromhex(raw_hex)
    except ValueError:
        return {
            "status": "skipped",
            "reason": "invalid_spend_bundle_hex",
            "operation_id": None,
        }

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
            return {
                "status": "skipped",
                "reason": f"push_tx_error:{exc}",
                "operation_id": None,
            }
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
        return {
            "status": "skipped",
            "reason": f"push_tx_runtime_error:{exc}",
            "operation_id": None,
        }


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def build_signed_spend_bundle(payload: dict[str, Any]) -> dict[str, Any]:
    """Build a signed spend bundle: coin discovery -> selection -> signing.

    Returns dict with 'status', 'reason', and 'spend_bundle_hex' on success.
    """
    key_id = str(payload.get("key_id", "")).strip()
    network = str(payload.get("network", "")).strip()
    receive_address = str(payload.get("receive_address", "")).strip()
    keyring_yaml_path = str(payload.get("keyring_yaml_path", "")).strip()
    asset_id = str(payload.get("asset_id", "")).strip().lower()

    if not key_id or not network or not receive_address:
        return {"status": "skipped", "reason": "missing_key_or_network_or_address"}
    if not keyring_yaml_path:
        return {"status": "skipped", "reason": "missing_keyring_yaml_path"}
    if asset_id not in {"xch", "1", ""}:
        return {"status": "skipped", "reason": "asset_not_supported_yet"}

    plan = payload.get("plan") or {}
    if not isinstance(plan, dict):
        return {"status": "skipped", "reason": "missing_plan"}
    op_type = str(plan.get("op_type", "")).strip()
    size_base_units = int(plan.get("size_base_units", 0))
    op_count = int(plan.get("op_count", 0))
    target_total = int(plan.get("target_total_base_units", 0))
    if target_total <= 0 and size_base_units > 0 and op_count > 0:
        target_total = size_base_units * op_count
        plan = dict(plan)
        plan["target_total_base_units"] = target_total
    if op_type not in {"split", "combine"} or target_total <= 0:
        return {"status": "skipped", "reason": "invalid_plan"}

    try:
        sdk = _import_sdk()
    except Exception as exc:
        return {"status": "skipped", "reason": f"wallet_sdk_import_error:{exc}"}

    coins = _list_unspent_xch_coins(sdk=sdk, receive_address=receive_address, network=network)
    if not coins:
        return {"status": "skipped", "reason": "no_unspent_xch_coins"}

    try:
        selected = sdk.select_coins(coins, target_total)
    except Exception as exc:
        return {"status": "skipped", "reason": f"coin_selection_failed:{exc}"}

    selected_coins = [
        {
            "coin_id": sdk.to_hex(c.coin_id()),
            "parent_coin_info": sdk.to_hex(c.parent_coin_info),
            "puzzle_hash": sdk.to_hex(c.puzzle_hash),
            "amount": int(c.amount),
        }
        for c in selected
    ]

    additions, additions_error = _build_additions_from_plan(
        plan=plan, selected_coins=selected_coins, receive_address=receive_address
    )
    if additions_error is not None:
        return {"status": "skipped", "reason": additions_error}

    sign_payload = {
        "selected_coins": selected_coins,
        "additions": additions,
    }

    spend_bundle_hex, error = _build_spend_bundle(
        sdk=sdk,
        payload=sign_payload,
        keyring_yaml_path=keyring_yaml_path,
        key_id=key_id,
        network=network,
        receive_address=receive_address,
    )
    if spend_bundle_hex is None:
        return {"status": "skipped", "reason": f"signing_failed:{error}"}

    return {
        "status": "executed",
        "reason": "signing_success",
        "spend_bundle_hex": spend_bundle_hex,
    }


def sign_and_broadcast(payload: dict[str, Any]) -> dict[str, Any]:
    """Build, sign, and broadcast a spend bundle.

    Used by the daemon coin-op execution path (WalletAdapter).
    """
    result = build_signed_spend_bundle(payload)
    if result.get("status") != "executed":
        return {
            "status": "skipped",
            "reason": result.get("reason", "signing_failed"),
            "operation_id": None,
        }

    spend_bundle_hex = str(result.get("spend_bundle_hex", ""))
    try:
        sdk = _import_sdk()
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"wallet_sdk_import_error:{exc}",
            "operation_id": None,
        }

    return _broadcast_spend_bundle(
        sdk=sdk,
        spend_bundle_hex=spend_bundle_hex,
        network=str(payload.get("network", "")).strip(),
    )

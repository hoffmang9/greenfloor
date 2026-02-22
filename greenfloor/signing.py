"""Unified signing module for GreenFloor.

Handles coin discovery, selection, spend-bundle construction, signing,
and optional broadcast. Used by both the daemon (coin-op execution via
WalletAdapter) and the manager (offer building via offer_builder_sdk).
"""

from __future__ import annotations

import importlib
import json
import os
from pathlib import Path
from typing import Any

from greenfloor.adapters.coinset import CoinsetAdapter

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


def _coinset_base_url(*, network: str) -> str:
    _ = network
    return os.getenv("GREENFLOOR_COINSET_BASE_URL", "").strip()


def _coinset_adapter(*, network: str) -> CoinsetAdapter:
    base_url = _coinset_base_url(network=network)
    return CoinsetAdapter(base_url or None, network=network)


def _to_coinset_hex(value: bytes) -> str:
    return f"0x{value.hex()}"


def _coin_from_record(*, sdk: Any, record: dict[str, Any]) -> Any | None:
    coin_data = record.get("coin")
    if not isinstance(coin_data, dict):
        return None
    parent_hex = str(coin_data.get("parent_coin_info", "")).strip()
    puzzle_hex = str(coin_data.get("puzzle_hash", "")).strip()
    amount_raw = coin_data.get("amount", 0)
    if not parent_hex or not puzzle_hex:
        return None
    try:
        return sdk.Coin(
            _hex_to_bytes(parent_hex),
            _hex_to_bytes(puzzle_hex),
            int(amount_raw),
        )
    except Exception:
        return None


def _spent_height_from_record(record: dict[str, Any]) -> int:
    spent_raw = record.get("spent_block_index", record.get("spent_height", 0))
    try:
        return int(spent_raw or 0)
    except (TypeError, ValueError):
        return 0


class _CoinSolutionView:
    def __init__(self, *, puzzle_reveal: bytes, solution: bytes) -> None:
        self.puzzle_reveal = puzzle_reveal
        self.solution = solution


# ---------------------------------------------------------------------------
# Coin discovery
# ---------------------------------------------------------------------------


def _list_unspent_xch_coins(*, sdk: Any, receive_address: str, network: str) -> list[Any]:
    try:
        address = sdk.Address.decode(receive_address)
        puzzle_hash = address.puzzle_hash
        records = _coinset_adapter(network=network).get_coin_records_by_puzzle_hash(
            puzzle_hash_hex=_to_coinset_hex(puzzle_hash),
            include_spent_coins=False,
        )
        coins: list[Any] = []
        for record in records:
            coin = _coin_from_record(sdk=sdk, record=record)
            if coin is not None:
                coins.append(coin)
        return coins
    except Exception:
        return []


def _list_unspent_cat_coins(
    *,
    sdk: Any,
    receive_address: str,
    network: str,
    asset_id: str,
) -> list[Any]:
    try:
        address = sdk.Address.decode(receive_address)
        inner_puzzle_hash = address.puzzle_hash
        asset_id_bytes = _hex_to_bytes(asset_id)
        cat_puzzle_hash = sdk.cat_puzzle_hash(asset_id_bytes, inner_puzzle_hash)
        coinset = _coinset_adapter(network=network)
        records = coinset.get_coin_records_by_puzzle_hash(
            puzzle_hash_hex=_to_coinset_hex(cat_puzzle_hash),
            include_spent_coins=False,
        )
        if not records:
            return []

        clvm = sdk.Clvm()
        cats: list[Any] = []
        for record in records:
            coin = _coin_from_record(sdk=sdk, record=record)
            if coin is None:
                continue

            parent_record = coinset.get_coin_record_by_name(
                coin_name_hex=_to_coinset_hex(coin.parent_coin_info)
            )
            if parent_record is None:
                continue
            parent_coin = _coin_from_record(sdk=sdk, record=parent_record)
            if parent_coin is None:
                continue
            parent_spent_height = _spent_height_from_record(parent_record)
            if parent_spent_height <= 0:
                continue

            parent_solution_record = coinset.get_puzzle_and_solution(
                coin_id_hex=_to_coinset_hex(parent_coin.coin_id()),
                height=parent_spent_height,
            )
            if parent_solution_record is None:
                continue
            puzzle_reveal_hex = str(parent_solution_record.get("puzzle_reveal", "")).strip()
            solution_hex = str(parent_solution_record.get("solution", "")).strip()
            if not puzzle_reveal_hex or not solution_hex:
                continue

            try:
                parent_coin_spend = _CoinSolutionView(
                    puzzle_reveal=_hex_to_bytes(puzzle_reveal_hex),
                    solution=_hex_to_bytes(solution_hex),
                )
                parent_puzzle = clvm.deserialize(parent_coin_spend.puzzle_reveal)
                parent_solution = clvm.deserialize(parent_coin_spend.solution)
            except Exception:
                continue
            try:
                parsed_children = parent_puzzle.parse_child_cats(parent_coin, parent_solution)
            except Exception:
                parsed_children = None
            if not parsed_children:
                continue
            for cat in parsed_children:
                child_coin = getattr(cat, "coin", None)
                if child_coin is None:
                    continue
                if sdk.to_hex(child_coin.coin_id()) == sdk.to_hex(coin.coin_id()):
                    cats.append(cat)
                    break
        return cats
    except Exception:
        return []


def _select_cats(cats: list[Any], target_total: int) -> list[Any]:
    sorted_cats = sorted(cats, key=lambda c: int(c.coin.amount))
    selected: list[Any] = []
    running = 0
    for cat in sorted_cats:
        selected.append(cat)
        running += int(cat.coin.amount)
        if running >= target_total:
            return selected
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
            p2_puzzle_hash_hex = str(item.get("p2_puzzle_hash", "")).strip()
            if p2_puzzle_hash_hex:
                selected_coin_puzzle_hashes.add(_hex_to_bytes(p2_puzzle_hash_hex))
            else:
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


def _asset_id_to_sdk_id(*, sdk: Any, asset_id: str) -> Any:
    raw = asset_id.strip().lower()
    if raw in {"", "xch", "1"}:
        return sdk.Id.xch()
    return sdk.Id.existing(_hex_to_bytes(raw))


def _build_offer_spend_bundle(
    *,
    sdk: Any,
    keyring_yaml_path: str,
    key_id: str,
    network: str,
    receive_address: str,
    offer_asset_id: str,
    offer_amount: int,
    request_asset_id: str,
    request_amount: int,
) -> tuple[str | None, str | None]:
    if offer_amount <= 0 or request_amount <= 0:
        return None, "invalid_offer_or_request_amount"

    receive_puzzle_hash = sdk.Address.decode(receive_address).puzzle_hash
    offer_asset = offer_asset_id.strip().lower()
    offered_selected_xch: list[Any] = []
    offered_selected_cats: list[Any] = []

    if offer_asset in {"", "xch", "1"}:
        xch_coins = _list_unspent_xch_coins(
            sdk=sdk,
            receive_address=receive_address,
            network=network,
        )
        if not xch_coins:
            return None, "no_unspent_offer_xch_coins"
        try:
            offered_selected_xch = sdk.select_coins(xch_coins, offer_amount)
        except Exception as exc:
            return None, f"offer_coin_selection_failed:{exc}"
    else:
        cat_coins = _list_unspent_cat_coins(
            sdk=sdk,
            receive_address=receive_address,
            network=network,
            asset_id=offer_asset,
        )
        if not cat_coins:
            return None, "no_unspent_offer_cat_coins"
        offered_selected_cats = _select_cats(cat_coins, offer_amount)
        if not offered_selected_cats:
            return None, "insufficient_offer_cat_coins"

    offered_total = 0
    selected_coin_entries: list[dict[str, Any]] = []
    for coin in offered_selected_xch:
        amount = int(coin.amount)
        offered_total += amount
        selected_coin_entries.append(
            {
                "coin_id": sdk.to_hex(coin.coin_id()),
                "parent_coin_info": sdk.to_hex(coin.parent_coin_info),
                "puzzle_hash": sdk.to_hex(coin.puzzle_hash),
                "amount": amount,
                "p2_puzzle_hash": sdk.to_hex(coin.puzzle_hash),
            }
        )
    for cat in offered_selected_cats:
        amount = int(cat.coin.amount)
        offered_total += amount
        selected_coin_entries.append(
            {
                "coin_id": sdk.to_hex(cat.coin.coin_id()),
                "parent_coin_info": sdk.to_hex(cat.coin.parent_coin_info),
                "puzzle_hash": sdk.to_hex(cat.coin.puzzle_hash),
                "amount": amount,
                "p2_puzzle_hash": sdk.to_hex(cat.info.p2_puzzle_hash),
            }
        )
    if offered_total < offer_amount:
        return None, "insufficient_offer_coin_total"

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
    selected_coin_puzzle_hashes = {
        _hex_to_bytes(str(item["p2_puzzle_hash"])) for item in selected_coin_entries
    }
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
        spends = sdk.Spends(clvm, receive_puzzle_hash)
        for coin in offered_selected_xch:
            spends.add_xch(coin)
        for cat in offered_selected_cats:
            spends.add_cat(cat)

        actions: list[Any] = [
            sdk.Action.send(
                _asset_id_to_sdk_id(sdk=sdk, asset_id=request_asset_id),
                receive_puzzle_hash,
                request_amount,
            )
        ]
        offer_change = offered_total - offer_amount
        if offer_change > 0:
            actions.append(
                sdk.Action.send(
                    _asset_id_to_sdk_id(sdk=sdk, asset_id=offer_asset_id),
                    receive_puzzle_hash,
                    offer_change,
                )
            )
        deltas = spends.apply(actions)
        finished = spends.prepare(deltas)
        for pending_spend in finished.pending_spends():
            coin = pending_spend.coin()
            try:
                signing_puzzle_hash = pending_spend.p2_puzzle_hash()
            except Exception:
                signing_puzzle_hash = coin.puzzle_hash
            synthetic_sk = synthetic_sk_by_puzzle_hash.get(signing_puzzle_hash)
            if synthetic_sk is None:
                return None, "missing_signing_key_for_pending_spend"
            delegated = clvm.delegated_spend(pending_spend.conditions())
            clvm.spend_standard_coin(coin, synthetic_sk.public_key(), delegated)
        coin_spends = clvm.coin_spends()
    except Exception as exc:
        return None, f"build_offer_spend_bundle_error:{exc}"

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
                synthetic_sk_chia = chia_rs.PrivateKey.from_bytes(sk.to_bytes())
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

    try:
        response = _coinset_adapter(network=network).push_tx(spend_bundle_hex=spend_bundle_hex)
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"push_tx_error:{exc}",
            "operation_id": None,
        }
    if not bool(response.get("success", False)):
        err = response.get("error") or "push_tx_rejected"
        return {"status": "skipped", "reason": str(err), "operation_id": None}
    tx_id = sdk.to_hex(spend_bundle.hash())
    return {
        "status": "executed",
        "reason": str(response.get("status", "submitted")),
        "operation_id": tx_id,
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
    plan = payload.get("plan") or {}
    if not isinstance(plan, dict):
        return {"status": "skipped", "reason": "missing_plan"}
    op_type = str(plan.get("op_type", "")).strip()

    if op_type == "offer":
        offer_asset_id = str(plan.get("offer_asset_id", asset_id)).strip().lower()
        request_asset_id = str(plan.get("request_asset_id", "")).strip().lower()
        offer_amount = int(plan.get("offer_amount", 0))
        request_amount = int(plan.get("request_amount", 0))
        if not request_asset_id:
            return {"status": "skipped", "reason": "missing_request_asset_id"}
        try:
            sdk = _import_sdk()
        except Exception as exc:
            return {"status": "skipped", "reason": f"wallet_sdk_import_error:{exc}"}
        spend_bundle_hex, error = _build_offer_spend_bundle(
            sdk=sdk,
            keyring_yaml_path=keyring_yaml_path,
            key_id=key_id,
            network=network,
            receive_address=receive_address,
            offer_asset_id=offer_asset_id,
            offer_amount=offer_amount,
            request_asset_id=request_asset_id,
            request_amount=request_amount,
        )
        if spend_bundle_hex is None:
            return {"status": "skipped", "reason": f"signing_failed:{error}"}
        return {
            "status": "executed",
            "reason": "signing_success",
            "spend_bundle_hex": spend_bundle_hex,
        }

    if asset_id not in {"xch", "1", ""}:
        return {"status": "skipped", "reason": "asset_not_supported_yet"}

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

"""Unified signing module for GreenFloor.

Handles coin discovery, selection, spend-bundle construction, signing,
and optional broadcast. Used by both the daemon (coin-op execution via
WalletAdapter) and the manager (offer building via offer_builder_sdk).
"""

from __future__ import annotations

import hashlib
import importlib
import json
import os
from typing import Any

from greenfloor.adapters.coinset import CoinsetAdapter

_AGG_SIG_ADDITIONAL_DATA_BY_NETWORK: dict[str, bytes] = {
    # Match chia-wallet-sdk consensus constants for AGG_SIG_ME domain separation.
    "mainnet": bytes.fromhex("ccd5bb71183532bff220ba46c268991a3ff07eb358e8255a65c30a2dce0e5fbb"),
    "testnet11": bytes.fromhex("37a90eb5185a9c4439a91ddc98bbadce7b4feba060d50116a067de66bf236615"),
}


def _int_to_clvm_bytes(value: int) -> bytes:
    if value <= 0:
        return b""
    size = (int(value).bit_length() + 7) // 8
    return int(value).to_bytes(size, "big", signed=False)


def _domain_bytes_for_agg_sig_kind(kind: str, agg_sig_me_additional_data: bytes) -> bytes | None:
    kind_l = kind.strip().lower()
    if kind_l == "unsafe":
        return None
    if kind_l == "me":
        return agg_sig_me_additional_data
    suffix_map = {
        "parent": 43,
        "puzzle": 44,
        "amount": 45,
        "puzzle_amount": 46,
        "parent_amount": 47,
        "parent_puzzle": 48,
    }
    suffix = suffix_map.get(kind_l)
    if suffix is None:
        return None
    hasher = hashlib.sha256()
    hasher.update(agg_sig_me_additional_data)
    hasher.update(bytes([suffix]))
    return hasher.digest()


def _extract_required_bls_targets_for_coin_spend(
    *,
    sdk: Any,
    coin_spend: Any,
    agg_sig_me_additional_data: bytes,
) -> list[tuple[bytes, bytes]]:
    clvm = sdk.Clvm()
    puzzle = clvm.deserialize(coin_spend.puzzle_reveal)
    solution = clvm.deserialize(coin_spend.solution)
    output = puzzle.run(solution, 11_000_000_000, False)
    conditions = output.value.to_list() or []

    coin = coin_spend.coin
    return _extract_required_bls_targets_for_conditions(
        conditions=conditions,
        coin=coin,
        agg_sig_me_additional_data=agg_sig_me_additional_data,
    )


def _extract_required_bls_targets_for_conditions(
    *,
    conditions: list[Any],
    coin: Any,
    agg_sig_me_additional_data: bytes,
) -> list[tuple[bytes, bytes]]:
    parent = bytes(coin.parent_coin_info)
    puzzle_hash = bytes(coin.puzzle_hash)
    amount = _int_to_clvm_bytes(int(coin.amount))
    coin_id = bytes(coin.coin_id())
    parser_specs = [
        ("parent", "parse_agg_sig_parent", parent),
        ("puzzle", "parse_agg_sig_puzzle", puzzle_hash),
        ("amount", "parse_agg_sig_amount", amount),
        ("puzzle_amount", "parse_agg_sig_puzzle_amount", puzzle_hash + amount),
        ("parent_amount", "parse_agg_sig_parent_amount", parent + amount),
        ("parent_puzzle", "parse_agg_sig_parent_puzzle", parent + puzzle_hash),
        ("unsafe", "parse_agg_sig_unsafe", b""),
        ("me", "parse_agg_sig_me", coin_id),
    ]

    targets: list[tuple[bytes, bytes]] = []
    for condition in conditions:
        for kind, parser_name, appended_info in parser_specs:
            parser = getattr(condition, parser_name, None)
            if parser is None:
                continue
            try:
                parsed = parser()
            except Exception:
                parsed = None
            if parsed is None:
                continue
            public_key = bytes(parsed.public_key.to_bytes())
            raw_message = bytes(parsed.message)
            domain = _domain_bytes_for_agg_sig_kind(kind, agg_sig_me_additional_data)
            full_message = raw_message + appended_info + (domain or b"")
            targets.append((public_key, full_message))
    return targets


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


def _import_greenfloor_native() -> Any:
    return importlib.import_module("greenfloor_native")


def _as_bytes(value: Any) -> bytes:
    if isinstance(value, (bytes, bytearray, memoryview)):
        return bytes(value)
    to_bytes = getattr(value, "to_bytes", None)
    if callable(to_bytes):
        raw = to_bytes()
        if isinstance(raw, (bytes, bytearray, memoryview)):
            return bytes(raw)
        raise TypeError("to_bytes did not return bytes-compatible data")
    to_dunder_bytes = getattr(value, "__bytes__", None)
    if callable(to_dunder_bytes):
        raw = to_dunder_bytes()
        if isinstance(raw, (bytes, bytearray, memoryview)):
            return bytes(raw)
        raise TypeError("__bytes__ did not return bytes-compatible data")
    raise TypeError("value cannot be converted to bytes")


def _coinset_base_url(*, network: str) -> str:
    base = os.getenv("GREENFLOOR_COINSET_BASE_URL", "").strip()
    if not base:
        return ""
    network_l = network.strip().lower()
    if network_l in {"testnet", "testnet11"}:
        allow_mainnet = os.getenv("GREENFLOOR_ALLOW_MAINNET_COINSET_FOR_TESTNET11", "").strip()
        if (
            "coinset.org" in base
            and "testnet11.api.coinset.org" not in base
            and allow_mainnet != "1"
        ):
            raise RuntimeError("coinset_base_url_mainnet_not_allowed_for_testnet11")
    return base


def _coinset_adapter(*, network: str) -> CoinsetAdapter:
    base_url = _coinset_base_url(network=network)
    require_testnet11 = network.strip().lower() in {"testnet", "testnet11"}
    return CoinsetAdapter(base_url or None, network=network, require_testnet11=require_testnet11)


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
    except Exception as exc:
        raise RuntimeError(f"xch_coin_discovery_failed:{exc}") from exc


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
        cat_puzzle_hash_hex = _to_coinset_hex(cat_puzzle_hash)
        records = coinset.get_coin_records_by_puzzle_hash(
            puzzle_hash_hex=cat_puzzle_hash_hex,
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
                parent_puzzle_program = clvm.deserialize(_hex_to_bytes(puzzle_reveal_hex))
                parent_solution = clvm.deserialize(_hex_to_bytes(solution_hex))
                parent_puzzle = parent_puzzle_program.puzzle()
            except Exception:
                continue
            try:
                parsed_children = parent_puzzle.parse_child_cats(parent_coin, parent_solution)
            except Exception:
                parsed_children = None
            if not parsed_children:
                continue
            matched_child = False
            for cat in parsed_children:
                child_coin = getattr(cat, "coin", None)
                if child_coin is None:
                    continue
                if sdk.to_hex(child_coin.coin_id()) == sdk.to_hex(coin.coin_id()):
                    cats.append(cat)
                    matched_child = True
                    break
            if not matched_child:
                continue
        return cats
    except Exception as exc:
        raise RuntimeError(f"cat_coin_discovery_failed:{exc}") from exc


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
    _ = keyring_yaml_path
    key_id_trimmed = key_id.strip()
    secret_key_hex_map_raw = os.getenv("GREENFLOOR_KEY_ID_SECRET_KEY_HEX_MAP_JSON", "").strip()
    if secret_key_hex_map_raw:
        try:
            secret_key_map = json.loads(secret_key_hex_map_raw)
        except json.JSONDecodeError:
            return None, "invalid_key_id_secret_key_hex_map_json"
        if isinstance(secret_key_map, dict):
            secret_key_hex_raw = str(secret_key_map.get(key_id_trimmed, "")).strip()
            if secret_key_hex_raw:
                try:
                    return _hex_to_bytes(secret_key_hex_raw), None
                except Exception as exc:
                    return None, f"invalid_secret_key_hex_for_key_id:{exc}"

    mnemonic_map_raw = os.getenv("GREENFLOOR_KEY_ID_MNEMONIC_MAP_JSON", "").strip()
    mnemonic = ""
    if mnemonic_map_raw:
        try:
            mnemonic_map = json.loads(mnemonic_map_raw)
        except json.JSONDecodeError:
            return None, "invalid_key_id_mnemonic_map_json"
        if isinstance(mnemonic_map, dict):
            mnemonic = str(mnemonic_map.get(key_id_trimmed, "")).strip()
    if not mnemonic:
        mnemonic = os.getenv("GREENFLOOR_WALLET_MNEMONIC", "").strip()
    if not mnemonic:
        mnemonic = os.getenv("TESTNET_WALLET_MNEMONIC", "").strip()
    if not mnemonic:
        return None, "missing_mnemonic_for_key_id"

    try:
        sdk = _import_sdk()
    except Exception as exc:
        return None, f"wallet_sdk_import_error:{exc}"
    try:
        mnemonic_obj = sdk.Mnemonic(mnemonic)
        seed = mnemonic_obj.to_seed("")
        master_sk = sdk.SecretKey.from_seed(seed)
        return master_sk.to_bytes(), None
    except Exception as exc:
        return None, f"mnemonic_to_master_key_error:{exc}"


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
        master_sk = sdk.SecretKey.from_bytes(bytes(master_private_key))
    except Exception as exc:
        return None, f"master_key_conversion_error:{exc}"

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

    synthetic_sk_by_puzzle_hash = _scan_synthetic_keys_for_puzzle_hashes(
        sdk=sdk,
        master_sk=master_sk,
        selected_coin_puzzle_hashes=selected_coin_puzzle_hashes,
    )
    if synthetic_sk_by_puzzle_hash is None:
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
                    None,
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
            for public_key, message in _extract_required_bls_targets_for_coin_spend(
                sdk=sdk,
                coin_spend=coin_spend,
                agg_sig_me_additional_data=additional_data,
            ):
                sk = sk_by_pk_bytes.get(public_key)
                if sk is None:
                    return None, "missing_private_key_for_agg_sig_target"
                signatures.append(sk.sign(message))
        if not signatures:
            return None, "no_agg_sig_targets_found"
        aggregate_sig = sdk.Signature.aggregate(signatures)
        spend_bundle = sdk.SpendBundle(coin_spends, aggregate_sig)
        return sdk.to_hex(spend_bundle.to_bytes()), None
    except Exception as exc:
        return None, f"sign_spend_bundle_error:{exc}"


def _asset_id_to_sdk_id(*, sdk: Any, asset_id: str) -> Any:
    raw = asset_id.strip().lower()
    if raw in {"", "xch", "txch", "1"}:
        return sdk.Id.xch()
    return sdk.Id.existing(_hex_to_bytes(raw))


def _from_input_spend_bundle_xch(
    *,
    sdk: Any,
    input_spend_bundle: Any,
    requested_payments_xch: list[Any],
) -> Any:
    native = _import_greenfloor_native()
    requested: list[tuple[bytes, list[tuple[bytes, int]]]] = []
    for notarized_payment in requested_payments_xch:
        payments: list[tuple[bytes, int]] = []
        for payment in notarized_payment.payments:
            payments.append((_as_bytes(payment.puzzle_hash), int(payment.amount)))
        requested.append((_as_bytes(notarized_payment.nonce), payments))
    spend_bundle_bytes = native.from_input_spend_bundle_xch(
        input_spend_bundle.to_bytes(),
        requested,
    )
    return sdk.SpendBundle.from_bytes(spend_bundle_bytes)


def _scan_synthetic_keys_for_puzzle_hashes(
    *,
    sdk: Any,
    master_sk: Any,
    selected_coin_puzzle_hashes: set[bytes],
) -> dict[bytes, Any] | None:
    derivation_scan_limit = int(os.getenv("GREENFLOOR_CHIA_KEYS_DERIVATION_SCAN_LIMIT", "200"))
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
            return synthetic_sk_by_puzzle_hash
    if not selected_coin_puzzle_hashes:
        return synthetic_sk_by_puzzle_hash
    return None


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
    dry_run: bool,
) -> tuple[str | None, str | None]:
    if offer_amount <= 0 or request_amount <= 0:
        return None, "invalid_offer_or_request_amount"

    receive_puzzle_hash = sdk.Address.decode(receive_address).puzzle_hash
    offer_asset = offer_asset_id.strip().lower()
    offered_selected_xch: list[Any] = []
    offered_selected_cats: list[Any] = []

    if offer_asset in {"", "xch", "txch", "1"}:
        try:
            xch_coins = _list_unspent_xch_coins(
                sdk=sdk,
                receive_address=receive_address,
                network=network,
            )
        except Exception as exc:
            return None, str(exc)
        if not xch_coins:
            return None, "no_unspent_offer_xch_coins"
        try:
            offered_selected_xch = sdk.select_coins(xch_coins, offer_amount)
        except Exception as exc:
            return None, f"offer_coin_selection_failed:{exc}"
    else:
        try:
            cat_coins = _list_unspent_cat_coins(
                sdk=sdk,
                receive_address=receive_address,
                network=network,
                asset_id=offer_asset,
            )
        except Exception as exc:
            return None, str(exc)
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
        master_sk = sdk.SecretKey.from_bytes(bytes(master_private_key))
    except Exception as exc:
        return None, f"master_key_conversion_error:{exc}"

    selected_coin_puzzle_hashes = {
        _hex_to_bytes(str(item["p2_puzzle_hash"])) for item in selected_coin_entries
    }
    synthetic_sk_by_puzzle_hash = _scan_synthetic_keys_for_puzzle_hashes(
        sdk=sdk,
        master_sk=master_sk,
        selected_coin_puzzle_hashes=selected_coin_puzzle_hashes,
    )
    if synthetic_sk_by_puzzle_hash is None:
        return None, "derivation_scan_failed_for_selected_coin"

    try:
        clvm = sdk.Clvm()
        spends = sdk.Spends(clvm, receive_puzzle_hash)
        for coin in offered_selected_xch:
            spends.add_xch(coin)
        for cat in offered_selected_cats:
            spends.add_cat(cat)

        request_asset = request_asset_id.strip().lower()
        if request_asset not in {"xch", "txch", "1", ""}:
            return None, "unsupported_request_asset_for_primary_offer_path"

        # Sage-style offer flow:
        # 1) build and sign maker input spends that send offered amount to settlement puzzle
        # 2) hand input spends + requested payments to sdk.from_input_spend_bundle(...)
        settlement_puzzle_hash = sdk.Constants.settlement_payment_hash()
        actions: list[Any] = [
            sdk.Action.send(
                _asset_id_to_sdk_id(sdk=sdk, asset_id=offer_asset_id),
                settlement_puzzle_hash,
                offer_amount,
                None,
            )
        ]
        offer_change = offered_total - offer_amount
        if offer_change > 0:
            actions.append(
                sdk.Action.send(
                    _asset_id_to_sdk_id(sdk=sdk, asset_id=offer_asset_id),
                    receive_puzzle_hash,
                    offer_change,
                    None,
                )
            )

        offered_coin_ids = sorted(
            [coin.coin_id() for coin in offered_selected_xch]
            + [cat.coin.coin_id() for cat in offered_selected_cats]
        )
        nonce_program = clvm.list([clvm.atom(coin_id) for coin_id in offered_coin_ids])
        offer_nonce = nonce_program.tree_hash()
        # Dexie expects requested payments to include an explicit memos list, even when empty.
        requested_payment = sdk.Payment(receive_puzzle_hash, request_amount, clvm.list([]))
        notarized_payment = sdk.NotarizedPayment(offer_nonce, [requested_payment])

        # The maker's offered coins must assert the puzzle announcement that the
        # settlement coin creates for this notarized payment. Without this assertion
        # the offer is not atomically linked and is rejected by Dexie as invalid.
        notarized_payment_hash = clvm.alloc(notarized_payment).tree_hash()
        spends.add_required_condition(
            clvm.assert_puzzle_announcement(
                hashlib.sha256(settlement_puzzle_hash + notarized_payment_hash).digest()
            )
        )

        deltas = spends.apply(actions)
        finished = spends.prepare(deltas)
        pending_cat_spends: list[Any] = []
        for pending_spend in finished.pending_spends():
            coin = pending_spend.coin()
            pending_cat = None
            try:
                pending_cat = pending_spend.as_cat()
            except Exception:
                pending_cat = None
            delegated = clvm.delegated_spend(pending_spend.conditions())
            if pending_cat is not None:
                try:
                    signing_puzzle_hash = pending_spend.p2_puzzle_hash()
                except Exception:
                    signing_puzzle_hash = getattr(
                        pending_cat.info, "p2_puzzle_hash", coin.puzzle_hash
                    )
                synthetic_sk = synthetic_sk_by_puzzle_hash.get(signing_puzzle_hash)
                if synthetic_sk is None:
                    return None, "missing_signing_key_for_pending_spend"
                cat_inner_spend = clvm.standard_spend(synthetic_sk.public_key(), delegated)
                pending_cat_spends.append(sdk.CatSpend(pending_cat, cat_inner_spend))
                continue
            try:
                signing_puzzle_hash = pending_spend.p2_puzzle_hash()
            except Exception:
                signing_puzzle_hash = coin.puzzle_hash
            synthetic_sk = synthetic_sk_by_puzzle_hash.get(signing_puzzle_hash)
            if synthetic_sk is None:
                return None, "missing_signing_key_for_pending_spend"
            clvm.spend_standard_coin(coin, synthetic_sk.public_key(), delegated)
        if pending_cat_spends:
            clvm.spend_cats(pending_cat_spends)
        coin_spends = clvm.coin_spends()
    except Exception as exc:
        return None, f"build_offer_spend_bundle_error:{exc}"

    try:
        signatures = []
        sk_by_pk_bytes: dict[bytes, Any] = {}
        for synthetic_sk in synthetic_sk_by_puzzle_hash.values():
            sk_by_pk_bytes[synthetic_sk.public_key().to_bytes()] = synthetic_sk
        for coin_spend in coin_spends:
            for public_key, message in _extract_required_bls_targets_for_coin_spend(
                sdk=sdk,
                coin_spend=coin_spend,
                agg_sig_me_additional_data=additional_data,
            ):
                sk = sk_by_pk_bytes.get(public_key)
                if sk is None:
                    return None, "missing_private_key_for_agg_sig_target"
                signatures.append(sk.sign(message))
        if not signatures:
            return None, "no_agg_sig_targets_found"
        aggregate_sig = sdk.Signature.aggregate(signatures)
        input_spend_bundle = sdk.SpendBundle(coin_spends, aggregate_sig)
        spend_bundle = _from_input_spend_bundle_xch(
            sdk=sdk,
            input_spend_bundle=input_spend_bundle,
            requested_payments_xch=[notarized_payment],
        )
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
            dry_run=bool(payload.get("dry_run", False)),
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

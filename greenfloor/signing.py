"""Unified signing module for GreenFloor.

Handles coin discovery, selection, spend-bundle construction, signing,
and optional broadcast. Used by both the daemon (coin-op execution via
WalletAdapter) and the manager (offer building via offer_builder_sdk).
"""

from __future__ import annotations

import hashlib
import importlib
import json
import logging
import os
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.adapters.coinset import CoinsetAdapter

_AGG_SIG_ADDITIONAL_DATA_BY_NETWORK: dict[str, bytes] = {
    # Match chia-wallet-sdk consensus constants for AGG_SIG_ME domain separation.
    "mainnet": bytes.fromhex("ccd5bb71183532bff220ba46c268991a3ff07eb358e8255a65c30a2dce0e5fbb"),
    "testnet11": bytes.fromhex("37a90eb5185a9c4439a91ddc98bbadce7b4feba060d50116a067de66bf236615"),
}

_XCH_LIKE_ASSETS: frozenset[str] = frozenset({"", "xch", "txch", "1"})
logger = logging.getLogger("greenfloor.signing")


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
    if isinstance(value, bytes | bytearray | memoryview):
        return bytes(value)
    to_bytes = getattr(value, "to_bytes", None)
    if callable(to_bytes):
        raw = to_bytes()
        if isinstance(raw, bytes | bytearray | memoryview):
            return bytes(raw)
        raise TypeError("to_bytes did not return bytes-compatible data")
    to_dunder_bytes = getattr(value, "__bytes__", None)
    if callable(to_dunder_bytes):
        raw = to_dunder_bytes()
        if isinstance(raw, bytes | bytearray | memoryview):
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
    coinset = _coinset_adapter(network=network)
    try:
        address = sdk.Address.decode(receive_address)
        inner_puzzle_hash = address.puzzle_hash
        asset_id_bytes = _hex_to_bytes(asset_id)
        cat_puzzle_hash = sdk.cat_puzzle_hash(asset_id_bytes, inner_puzzle_hash)
        cat_puzzle_hash_hex = _to_coinset_hex(cat_puzzle_hash)
        records = coinset.get_coin_records_by_puzzle_hash(
            puzzle_hash_hex=cat_puzzle_hash_hex,
            include_spent_coins=False,
        )
        if not records:
            return []
        cats: list[Any] = []
        for record in records:
            cat = _cat_from_coin_record(sdk=sdk, coinset=coinset, record=record)
            if cat is not None:
                cats.append(cat)
        return cats
    except Exception as exc:
        raise RuntimeError(f"cat_coin_discovery_failed:{exc}") from exc


def _cat_from_coin_record(
    *, sdk: Any, coinset: CoinsetAdapter, record: dict[str, Any]
) -> Any | None:
    try:
        coin = _coin_from_record(sdk=sdk, record=record)
        if coin is None:
            return None
        parent_record = coinset.get_coin_record_by_name(
            coin_name_hex=_to_coinset_hex(coin.parent_coin_info)
        )
        if parent_record is None:
            return None
        parent_coin = _coin_from_record(sdk=sdk, record=parent_record)
        if parent_coin is None:
            return None
        parent_spent_height = _spent_height_from_record(parent_record)
        if parent_spent_height <= 0:
            return None
        parent_solution_record = coinset.get_puzzle_and_solution(
            coin_id_hex=_to_coinset_hex(parent_coin.coin_id()),
            height=parent_spent_height,
        )
        if parent_solution_record is None:
            return None
        puzzle_reveal_hex = str(parent_solution_record.get("puzzle_reveal", "")).strip()
        solution_hex = str(parent_solution_record.get("solution", "")).strip()
        if not puzzle_reveal_hex or not solution_hex:
            return None
        clvm = sdk.Clvm()
        parent_puzzle_program = clvm.deserialize(_hex_to_bytes(puzzle_reveal_hex))
        parent_solution = clvm.deserialize(_hex_to_bytes(solution_hex))
        parent_puzzle = parent_puzzle_program.puzzle()
    except Exception:
        return None
    try:
        parsed_children = parent_puzzle.parse_child_cats(parent_coin, parent_solution)
    except Exception:
        parsed_children = None
    if not parsed_children:
        return None
    coin_id_hex = sdk.to_hex(coin.coin_id())
    for cat in parsed_children:
        child_coin = getattr(cat, "coin", None)
        if child_coin is None:
            continue
        if sdk.to_hex(child_coin.coin_id()) == coin_id_hex:
            return cat
    return None


def _list_unspent_cat_coins_by_ids(
    *,
    sdk: Any,
    network: str,
    coin_ids: list[str],
) -> list[Any]:
    coinset = _coinset_adapter(network=network)
    cats: list[Any] = []
    seen_ids: set[str] = set()
    for raw_coin_id in coin_ids:
        clean_coin_id = str(raw_coin_id).strip().lower()
        if clean_coin_id.startswith("0x"):
            clean_coin_id = clean_coin_id[2:]
        if len(clean_coin_id) != 64:
            continue
        if clean_coin_id in seen_ids:
            continue
        seen_ids.add(clean_coin_id)
        try:
            record = coinset.get_coin_record_by_name(coin_name_hex=f"0x{clean_coin_id}")
        except Exception:
            continue
        if not isinstance(record, dict):
            continue
        if _spent_height_from_record(record) > 0:
            continue
        cat = _cat_from_coin_record(sdk=sdk, coinset=coinset, record=record)
        if cat is not None:
            cats.append(cat)
    return cats


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
# Signing context
# ---------------------------------------------------------------------------


def _load_signing_context(
    *,
    sdk: Any,
    keyring_yaml_path: str,
    key_id: str,
    network: str,
    selected_coin_puzzle_hashes: set[bytes],
) -> tuple[dict[bytes, Any] | None, bytes | None, str | None]:
    """Load key material and derive synthetic keys for selected coins.

    Returns (synthetic_sk_by_puzzle_hash, additional_data, None) on success,
    or (None, None, error_reason) on failure.
    """
    master_private_key, key_error = _load_master_private_key(keyring_yaml_path, key_id)
    if key_error:
        return None, None, key_error
    if master_private_key is None:
        return None, None, "key_secrets_unavailable"

    additional_data = _AGG_SIG_ADDITIONAL_DATA_BY_NETWORK.get(network)
    if additional_data is None:
        return None, None, "unsupported_network_for_signing"

    try:
        master_sk = sdk.SecretKey.from_bytes(bytes(master_private_key))
    except Exception as exc:
        return None, None, f"master_key_conversion_error:{exc}"

    synthetic_sk_by_puzzle_hash = _scan_synthetic_keys_for_puzzle_hashes(
        sdk=sdk,
        master_sk=master_sk,
        selected_coin_puzzle_hashes=selected_coin_puzzle_hashes,
    )
    if synthetic_sk_by_puzzle_hash is None:
        return None, None, "derivation_scan_failed_for_selected_coin"

    return synthetic_sk_by_puzzle_hash, additional_data, None


# ---------------------------------------------------------------------------
# Spend-bundle construction & signing
# ---------------------------------------------------------------------------


def _sign_and_aggregate(
    *,
    sdk: Any,
    coin_spends: list[Any],
    synthetic_sk_by_puzzle_hash: dict[bytes, Any],
    additional_data: bytes,
    allow_empty_signatures: bool = False,
) -> tuple[Any | None, str | None]:
    """Collect AGG_SIG targets from coin spends, sign, and aggregate."""
    sk_by_pk_bytes: dict[bytes, Any] = {}
    for synthetic_sk in synthetic_sk_by_puzzle_hash.values():
        sk_by_pk_bytes[synthetic_sk.public_key().to_bytes()] = synthetic_sk
    signatures = []
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
        if allow_empty_signatures:
            return sdk.Signature.infinity(), None
        return None, "no_agg_sig_targets_found"
    return sdk.Signature.aggregate(signatures), None


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

    synthetic_sk_by_puzzle_hash, additional_data, ctx_err = _load_signing_context(
        sdk=sdk,
        keyring_yaml_path=keyring_yaml_path,
        key_id=key_id,
        network=network,
        selected_coin_puzzle_hashes=selected_coin_puzzle_hashes,
    )
    if ctx_err is not None:
        return None, ctx_err
    assert synthetic_sk_by_puzzle_hash is not None
    assert additional_data is not None

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
        aggregate_sig, sign_err = _sign_and_aggregate(
            sdk=sdk,
            coin_spends=coin_spends,
            synthetic_sk_by_puzzle_hash=synthetic_sk_by_puzzle_hash,
            additional_data=additional_data,
        )
        if sign_err is not None:
            return None, sign_err
        spend_bundle = sdk.SpendBundle(coin_spends, aggregate_sig)
        return sdk.to_hex(spend_bundle.to_bytes()), None
    except Exception as exc:
        return None, f"sign_spend_bundle_error:{exc}"


def _asset_id_to_sdk_id(*, sdk: Any, asset_id: str) -> Any:
    raw = asset_id.strip().lower()
    if raw in _XCH_LIKE_ASSETS:
        return sdk.Id.xch()
    return sdk.Id.existing(_hex_to_bytes(raw))


def _normalize_hex_32(value: str) -> str:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if len(raw) != 64:
        return ""
    if not all(ch in "0123456789abcdef" for ch in raw):
        return ""
    return raw


def _normalize_hex_any(value: str) -> str:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if not raw:
        return ""
    if len(raw) % 2 != 0:
        return ""
    if not all(ch in "0123456789abcdef" for ch in raw):
        return ""
    return raw


def _extract_wallet_keys_from_connection(connection: Any) -> list[dict[str, str]]:
    if not isinstance(connection, dict):
        return []
    edges = connection.get("edges")
    if not isinstance(edges, list):
        return []
    results: list[dict[str, str]] = []
    for edge in edges:
        node = edge.get("node") if isinstance(edge, dict) else None
        if not isinstance(node, dict):
            continue
        pubkey_hex = _normalize_hex_any(str(node.get("publicKey", "")))
        curve = str(node.get("curve", "")).strip().upper()
        if not pubkey_hex or not curve:
            continue
        results.append({"public_key_hex": pubkey_hex, "curve": curve})
    return results


def _resolve_local_vault_offer_context(
    *,
    sdk: Any,
    payload: dict[str, Any],
    network: str,
) -> tuple[dict[str, bytes] | None, str | None]:
    kms_key_id = str(payload.get("cloud_wallet_kms_key_id", "")).strip()
    if not kms_key_id:
        return None, None
    base_url = str(payload.get("cloud_wallet_base_url", "")).strip()
    user_key_id = str(payload.get("cloud_wallet_user_key_id", "")).strip()
    private_key_pem_path = str(payload.get("cloud_wallet_private_key_pem_path", "")).strip()
    vault_id = str(payload.get("cloud_wallet_vault_id", "")).strip()
    if not base_url or not user_key_id or not private_key_pem_path or not vault_id:
        return None, "missing_cloud_wallet_credentials_for_local_vault_offer"
    adapter = CloudWalletAdapter(
        CloudWalletConfig(
            base_url=base_url,
            user_key_id=user_key_id,
            private_key_pem_path=private_key_pem_path,
            vault_id=vault_id,
            network=network,
            kms_key_id=kms_key_id,
            kms_region=str(payload.get("cloud_wallet_kms_region", "")).strip() or None,
            kms_public_key_hex=str(payload.get("cloud_wallet_kms_public_key_hex", "")).strip()
            or None,
        )
    )
    snapshot = adapter.get_vault_custody_snapshot()
    if not isinstance(snapshot, dict):
        return None, "vault_custody_snapshot_unavailable"
    launcher_id_hex = _normalize_hex_32(str(snapshot.get("vaultLauncherId", "")))
    if not launcher_id_hex:
        return None, "vault_launcher_id_missing_or_invalid"
    try:
        custody_threshold = int(snapshot.get("custodyThreshold", 0))
        recovery_threshold = int(snapshot.get("recoveryThreshold", 0))
        clawback_timelock = int(snapshot.get("recoveryClawbackTimelock", 0))
    except (TypeError, ValueError):
        return None, "vault_threshold_or_timelock_invalid"
    custody_keys = _extract_wallet_keys_from_connection(snapshot.get("custodyKeys"))
    recovery_keys = _extract_wallet_keys_from_connection(snapshot.get("recoveryKeys"))
    if len(custody_keys) == 0 or len(recovery_keys) == 0:
        return None, "unsupported_vault_signer_cardinality_for_local_offer"
    if custody_threshold <= 0 or custody_threshold > len(custody_keys):
        return None, "unsupported_vault_threshold_for_local_offer"
    if recovery_threshold <= 0 or recovery_threshold > len(recovery_keys):
        return None, "unsupported_vault_threshold_for_local_offer"
    if clawback_timelock <= 0:
        return None, "invalid_vault_recovery_timelock"

    def _member_hash_for_key(config: Any, key: dict[str, str]) -> bytes:
        curve = str(key.get("curve", "")).strip().upper()
        key_bytes = _hex_to_bytes(str(key.get("public_key_hex", "")))
        if curve == "SECP256R1":
            return bytes(
                sdk.r1_member_hash(
                    config,
                    sdk.R1PublicKey.from_bytes(key_bytes),
                    True,
                )
            )
        if curve == "SECP256K1":
            return bytes(
                sdk.k1_member_hash(
                    config,
                    sdk.K1PublicKey.from_bytes(key_bytes),
                    True,
                )
            )
        if curve == "WEBAUTHN":
            return bytes(
                sdk.passkey_member_hash(
                    config,
                    sdk.R1PublicKey.from_bytes(key_bytes),
                    True,
                )
            )
        if curve == "BLS12_381":
            # BLS member hash does not use fast-forward in this path.
            return bytes(
                sdk.bls_member_hash(
                    config,
                    sdk.PublicKey.from_bytes(key_bytes),
                    False,
                )
            )
        raise RuntimeError(f"unsupported_curve:{curve}")

    launcher_id = _hex_to_bytes(launcher_id_hex)
    member_config = sdk.MemberConfig()
    custody_hashes: list[bytes] = []
    for key in custody_keys:
        try:
            custody_hashes.append(_member_hash_for_key(member_config, key))
        except Exception as exc:
            return None, f"unsupported_vault_curve_for_local_offer:{exc}"
    custody_hashes.sort()
    if len(custody_hashes) == 1:
        custody_hash = custody_hashes[0]
    else:
        custody_hash = bytes(sdk.m_of_n_hash(member_config, custody_threshold, custody_hashes))
    clvm = sdk.Clvm()
    timelock = sdk.timelock_restriction(clawback_timelock)
    member_validator_list_hash = sdk.tree_hash_pair(
        timelock.puzzle_hash,
        clvm.nil().tree_hash(),
    )
    delegated_puzzle_validator_list_hash = clvm.nil().tree_hash()
    recovery_restrictions = [
        sdk.force_1_of_2_restriction(
            custody_hash,
            0,
            member_validator_list_hash,
            delegated_puzzle_validator_list_hash,
        ),
        *sdk.prevent_vault_side_effects_restriction(),
    ]
    recovery_config = member_config.with_restrictions(recovery_restrictions)
    recovery_hashes: list[bytes] = []
    for key in recovery_keys:
        try:
            recovery_hashes.append(_member_hash_for_key(member_config, key))
        except Exception as exc:
            return None, f"unsupported_vault_curve_for_local_offer:{exc}"
    recovery_hashes.sort()
    if len(recovery_hashes) == 1:
        try:
            recovery_hash = _member_hash_for_key(recovery_config, recovery_keys[0])
        except Exception as exc:
            return None, f"unsupported_vault_curve_for_local_offer:{exc}"
    else:
        recovery_hash = bytes(
            sdk.m_of_n_hash(
                recovery_config,
                recovery_threshold,
                recovery_hashes,
            )
        )
    inner_puzzle_hash = bytes(
        sdk.m_of_n_hash(
            member_config.with_top_level(True),
            1,
            [custody_hash, recovery_hash],
        )
    )
    p2_singleton_message_hash = bytes(
        sdk.singleton_member_hash(
            sdk.MemberConfig().with_top_level(True),
            launcher_id,
            False,
        )
    )
    return {
        "launcher_id": launcher_id,
        "inner_puzzle_hash": inner_puzzle_hash,
        "p2_singleton_message_hash": p2_singleton_message_hash,
    }, None


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


def _from_input_spend_bundle(
    *,
    sdk: Any,
    input_spend_bundle: Any,
    requested_payments_xch: list[Any] | None = None,
    requested_payments_cats: dict[str, list[Any]] | None = None,
) -> Any:
    native = _import_greenfloor_native()
    requested_xch: list[tuple[bytes, list[tuple[bytes, int]]]] = []
    for notarized_payment in requested_payments_xch or []:
        payments: list[tuple[bytes, int]] = []
        for payment in notarized_payment.payments:
            payments.append((_as_bytes(payment.puzzle_hash), int(payment.amount)))
        requested_xch.append((_as_bytes(notarized_payment.nonce), payments))

    if hasattr(native, "from_input_spend_bundle"):
        requested_cats: list[tuple[bytes, bytes, list[tuple[bytes, int]]]] = []
        for asset_id_hex, notarized_list in (requested_payments_cats or {}).items():
            asset_id_bytes = _hex_to_bytes(asset_id_hex)
            for notarized_payment in notarized_list:
                payments: list[tuple[bytes, int]] = []
                for payment in notarized_payment.payments:
                    payments.append((_as_bytes(payment.puzzle_hash), int(payment.amount)))
                requested_cats.append(
                    (
                        asset_id_bytes,
                        _as_bytes(notarized_payment.nonce),
                        payments,
                    )
                )
        spend_bundle_bytes = native.from_input_spend_bundle(
            input_spend_bundle.to_bytes(),
            requested_xch,
            requested_cats,
        )
        return sdk.SpendBundle.from_bytes(spend_bundle_bytes)

    if requested_payments_cats:
        raise RuntimeError("from_input_spend_bundle_missing_cat_support")
    spend_bundle_bytes = native.from_input_spend_bundle_xch(
        input_spend_bundle.to_bytes(),
        requested_xch,
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
    offer_coin_ids: list[str] | None = None,
    payload: dict[str, Any] | None = None,
) -> tuple[str | None, str | None]:
    if offer_amount <= 0 or request_amount <= 0:
        return None, "invalid_offer_or_request_amount"
    payload = payload or {}

    receive_puzzle_hash = sdk.Address.decode(receive_address).puzzle_hash
    offer_asset = offer_asset_id.strip().lower()
    offered_selected_xch: list[Any] = []
    offered_selected_cats: list[Any] = []
    discovered_coin_count = 0
    selected_coin_count = 0
    selected_total = 0
    selection_error_reason: str | None = None

    if offer_asset in _XCH_LIKE_ASSETS:
        try:
            xch_coins = _list_unspent_xch_coins(
                sdk=sdk,
                receive_address=receive_address,
                network=network,
            )
        except Exception as exc:
            return None, str(exc)
        discovered_coin_count = len(xch_coins)
        if not xch_coins:
            logger.info(
                "offer_coin_discovery network=%s receive_address=%s offer_asset_id=%s "
                "request_asset_id=%s discovered_coin_count=%s selected_coin_count=%s "
                "selected_total=%s outcome=%s",
                network,
                receive_address,
                offer_asset_id,
                request_asset_id,
                discovered_coin_count,
                selected_coin_count,
                selected_total,
                "no_unspent_offer_xch_coins",
            )
            return None, "no_unspent_offer_xch_coins"
        try:
            offered_selected_xch = sdk.select_coins(xch_coins, offer_amount)
        except Exception as exc:
            selection_error_reason = f"offer_coin_selection_failed:{exc}"
            logger.info(
                "offer_coin_discovery network=%s receive_address=%s offer_asset_id=%s "
                "request_asset_id=%s discovered_coin_count=%s selected_coin_count=%s "
                "selected_total=%s outcome=%s",
                network,
                receive_address,
                offer_asset_id,
                request_asset_id,
                discovered_coin_count,
                selected_coin_count,
                selected_total,
                selection_error_reason,
            )
            return None, f"offer_coin_selection_failed:{exc}"
        selected_coin_count = len(offered_selected_xch)
        selected_total = sum(int(coin.amount) for coin in offered_selected_xch)
    else:
        try:
            if offer_coin_ids:
                cat_coins = _list_unspent_cat_coins_by_ids(
                    sdk=sdk,
                    network=network,
                    coin_ids=offer_coin_ids,
                )
            else:
                cat_coins = _list_unspent_cat_coins(
                    sdk=sdk,
                    receive_address=receive_address,
                    network=network,
                    asset_id=offer_asset,
                )
        except Exception as exc:
            return None, str(exc)
        discovered_coin_count = len(cat_coins)
        if not cat_coins:
            logger.info(
                "offer_coin_discovery network=%s receive_address=%s offer_asset_id=%s "
                "request_asset_id=%s discovered_coin_count=%s selected_coin_count=%s "
                "selected_total=%s outcome=%s",
                network,
                receive_address,
                offer_asset_id,
                request_asset_id,
                discovered_coin_count,
                selected_coin_count,
                selected_total,
                "no_unspent_offer_cat_coins",
            )
            return None, "no_unspent_offer_cat_coins"
        offered_selected_cats = _select_cats(cat_coins, offer_amount)
        if not offered_selected_cats:
            selection_error_reason = "insufficient_offer_cat_coins"
            logger.info(
                "offer_coin_discovery network=%s receive_address=%s offer_asset_id=%s "
                "request_asset_id=%s discovered_coin_count=%s selected_coin_count=%s "
                "selected_total=%s outcome=%s",
                network,
                receive_address,
                offer_asset_id,
                request_asset_id,
                discovered_coin_count,
                selected_coin_count,
                selected_total,
                selection_error_reason,
            )
            return None, "insufficient_offer_cat_coins"
        selected_coin_count = len(offered_selected_cats)
        selected_total = sum(int(cat.coin.amount) for cat in offered_selected_cats)

    logger.info(
        "offer_coin_discovery network=%s receive_address=%s offer_asset_id=%s "
        "request_asset_id=%s discovered_coin_count=%s selected_coin_count=%s "
        "selected_total=%s outcome=%s",
        network,
        receive_address,
        offer_asset_id,
        request_asset_id,
        discovered_coin_count,
        selected_coin_count,
        selected_total,
        "selected",
    )

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

    selected_coin_puzzle_hashes = {
        _hex_to_bytes(str(item["p2_puzzle_hash"])) for item in selected_coin_entries
    }
    vault_ctx, vault_ctx_error = _resolve_local_vault_offer_context(
        sdk=sdk,
        payload=payload,
        network=network,
    )
    if vault_ctx_error is not None:
        return None, vault_ctx_error
    if vault_ctx is not None:
        synthetic_sk_by_puzzle_hash = {}
        additional_data = _AGG_SIG_ADDITIONAL_DATA_BY_NETWORK.get(network)
        if additional_data is None:
            return None, "unsupported_network_for_signing"
    else:
        synthetic_sk_by_puzzle_hash, additional_data, ctx_err = _load_signing_context(
            sdk=sdk,
            keyring_yaml_path=keyring_yaml_path,
            key_id=key_id,
            network=network,
            selected_coin_puzzle_hashes=selected_coin_puzzle_hashes,
        )
        if ctx_err is not None:
            return None, ctx_err
        assert synthetic_sk_by_puzzle_hash is not None
        assert additional_data is not None

    try:
        clvm = sdk.Clvm()
        spends = sdk.Spends(clvm, receive_puzzle_hash)
        for coin in offered_selected_xch:
            spends.add_xch(coin)
        for cat in offered_selected_cats:
            spends.add_cat(cat)

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
        request_asset = request_asset_id.strip().lower()
        # Dexie expects requested payments to include an explicit memos list, even when empty.
        if request_asset in _XCH_LIKE_ASSETS:
            requested_payment = sdk.Payment(receive_puzzle_hash, request_amount, clvm.list([]))
            requested_payment_map_xch = [sdk.NotarizedPayment(offer_nonce, [requested_payment])]
            requested_payment_map_cats: dict[str, list[Any]] = {}
        else:
            requested_payment = sdk.Payment(
                receive_puzzle_hash,
                request_amount,
                clvm.alloc([receive_puzzle_hash]),
            )
            requested_payment_map_xch = []
            requested_payment_map_cats = {
                request_asset: [sdk.NotarizedPayment(offer_nonce, [requested_payment])]
            }

        # The maker's offered coins must assert the puzzle announcement that the
        # settlement coin creates for this notarized payment. Without this assertion
        # the offer is not atomically linked and is rejected by Dexie as invalid.
        if request_asset in _XCH_LIKE_ASSETS:
            settlement_notarized_payment = requested_payment_map_xch[0]
        else:
            settlement_notarized_payment = requested_payment_map_cats[request_asset][0]
        notarized_payment_hash = clvm.alloc(settlement_notarized_payment).tree_hash()
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
                p2_puzzle_hash = None
                try:
                    p2_puzzle_hash = pending_spend.p2_puzzle_hash()
                except Exception:
                    p2_puzzle_hash = getattr(pending_cat.info, "p2_puzzle_hash", coin.puzzle_hash)
                if (
                    vault_ctx is not None
                    and isinstance(p2_puzzle_hash, bytes)
                    and bytes(p2_puzzle_hash) == vault_ctx["p2_singleton_message_hash"]
                ):
                    dummy_coin = sdk.Coin(
                        sdk.tree_hash_atom(b""),
                        sdk.tree_hash_atom(b""),
                        1,
                    )
                    mips = clvm.mips_spend(dummy_coin, delegated)
                    mips.singleton_member(
                        sdk.MemberConfig().with_top_level(True),
                        vault_ctx["launcher_id"],
                        False,
                        vault_ctx["inner_puzzle_hash"],
                        1,
                    )
                    cat_inner_spend = mips.spend(vault_ctx["p2_singleton_message_hash"])
                else:
                    if not isinstance(p2_puzzle_hash, bytes):
                        return None, "missing_p2_puzzle_hash_for_pending_spend"
                    synthetic_sk = synthetic_sk_by_puzzle_hash.get(bytes(p2_puzzle_hash))
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
        aggregate_sig, sign_err = _sign_and_aggregate(
            sdk=sdk,
            coin_spends=coin_spends,
            synthetic_sk_by_puzzle_hash=synthetic_sk_by_puzzle_hash,
            additional_data=additional_data,
            allow_empty_signatures=vault_ctx is not None,
        )
        if sign_err is not None:
            return None, sign_err
        input_spend_bundle = sdk.SpendBundle(coin_spends, aggregate_sig)
        spend_bundle = _from_input_spend_bundle(
            sdk=sdk,
            input_spend_bundle=input_spend_bundle,
            requested_payments_xch=requested_payment_map_xch,
            requested_payments_cats=requested_payment_map_cats,
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
        raw_offer_coin_ids = plan.get("offer_coin_ids", [])
        offer_coin_ids = (
            [str(value).strip().lower() for value in raw_offer_coin_ids if str(value).strip()]
            if isinstance(raw_offer_coin_ids, list)
            else []
        )
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
            offer_coin_ids=offer_coin_ids,
            payload=payload,
        )
        if spend_bundle_hex is None:
            return {"status": "skipped", "reason": f"signing_failed:{error}"}
        return {
            "status": "executed",
            "reason": "signing_success",
            "spend_bundle_hex": spend_bundle_hex,
        }

    if asset_id not in _XCH_LIKE_ASSETS:
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

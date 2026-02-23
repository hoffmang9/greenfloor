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
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from greenfloor.adapters.coinset import CoinsetAdapter

_AGG_SIG_ADDITIONAL_DATA_BY_NETWORK: dict[str, bytes] = {
    "mainnet": bytes.fromhex("37a90eb5185a9c4439a91ddc98bbadce7b4feba060d50116a067de66bf236615"),
    "testnet11": bytes.fromhex("b0a306abe27407130586c8e13d06dc057d4538c201dbd36c8f8c481f5e51af5c"),
}


def _is_signing_debug_enabled() -> bool:
    return os.getenv("GREENFLOOR_SIGNING_DEBUG", "").strip() == "1"


def _redact_address(value: str) -> str:
    raw = value.strip()
    if len(raw) <= 14:
        return raw
    return f"{raw[:8]}...{raw[-6:]}"


def _redact_hex(value: str) -> str:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if len(raw) <= 16:
        return raw
    return f"{raw[:8]}...{raw[-8:]}"


def _debug_signing(event: str, **payload: Any) -> None:
    if not _is_signing_debug_enabled():
        return
    message = {"component": "signing", "event": event, **payload}
    try:
        sys.stderr.write(f"{json.dumps(message, sort_keys=True)}\n")
    except Exception:
        # Debug logging must never interfere with signing flow.
        pass


def _capture_cat_parse_case(case: dict[str, Any]) -> None:
    target_dir = os.getenv("GREENFLOOR_CAT_PARSE_CAPTURE_DIR", "").strip()
    if not target_dir:
        return
    try:
        output_dir = Path(target_dir)
        output_dir.mkdir(parents=True, exist_ok=True)
        case_id = str(case.get("case_id", "")).strip() or "unknown"
        safe_case_id = "".join(ch for ch in case_id if ch.isalnum() or ch in {"-", "_"})
        if not safe_case_id:
            safe_case_id = "unknown"
        timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%S.%fZ")
        out_path = output_dir / f"{timestamp}-{safe_case_id}.json"
        out_path.write_text(json.dumps(case, sort_keys=True, indent=2), encoding="utf-8")
        _debug_signing(
            "cat_coin_parse_case_captured",
            capture_path=str(out_path),
            case_id=safe_case_id,
        )
    except Exception as exc:
        _debug_signing(
            "cat_coin_parse_case_capture_error",
            error=str(exc),
        )


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


def _debug_probe_parent_create_coin_outputs(
    *,
    sdk: Any,
    parent_coin_spend: _CoinSolutionView,
    parent_coin: Any,
    child_coin: Any,
    network: str,
    asset_id: str,
    record_index: int,
) -> None:
    if not _is_signing_debug_enabled():
        return
    try:
        clvm = sdk.Clvm()
        parent_puzzle = clvm.deserialize(parent_coin_spend.puzzle_reveal)
        parent_solution = clvm.deserialize(parent_coin_spend.solution)
        output = parent_puzzle.run(parent_solution, 11_000_000_000, False)
        conditions = output.value.to_list() or []
        outputs: list[tuple[bytes, int]] = []
        for condition in conditions:
            try:
                create_coin = condition.parse_create_coin()
            except Exception:
                create_coin = None
            if create_coin is None:
                continue
            outputs.append((bytes(create_coin.puzzle_hash), int(create_coin.amount)))
        target_child_coin_id = sdk.to_hex(child_coin.coin_id())
        target_child_puzzle_hash = sdk.to_hex(child_coin.puzzle_hash)
        target_child_amount = int(child_coin.amount)
        matched_by_coin_id = False
        matched_by_fields = False
        samples: list[dict[str, Any]] = []
        for idx, (puzzle_hash, amount) in enumerate(outputs):
            created_coin = sdk.Coin(parent_coin.coin_id(), puzzle_hash, amount)
            created_coin_id = sdk.to_hex(created_coin.coin_id())
            created_puzzle_hash = sdk.to_hex(puzzle_hash)
            if created_coin_id == target_child_coin_id:
                matched_by_coin_id = True
            if created_puzzle_hash == target_child_puzzle_hash and amount == target_child_amount:
                matched_by_fields = True
            if idx < 5:
                samples.append(
                    {
                        "created_coin_id": _redact_hex(created_coin_id),
                        "created_puzzle_hash": _redact_hex(created_puzzle_hash),
                        "created_amount": int(amount),
                    }
                )
        _debug_signing(
            "cat_coin_parent_create_coin_probe",
            network=network,
            asset_id=asset_id,
            record_index=record_index,
            target_coin_id=_redact_hex(target_child_coin_id),
            target_puzzle_hash=_redact_hex(target_child_puzzle_hash),
            target_amount=target_child_amount,
            create_coin_count=len(outputs),
            matched_by_coin_id=matched_by_coin_id,
            matched_by_fields=matched_by_fields,
            sample_create_coin_outputs=samples,
        )
    except Exception as exc:
        _debug_signing(
            "cat_coin_parent_create_coin_probe_error",
            network=network,
            asset_id=asset_id,
            record_index=record_index,
            error=str(exc),
        )


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
    base = os.getenv("GREENFLOOR_COINSET_BASE_URL", "").strip()
    if not base:
        _debug_signing(
            "coinset_base_url_resolved",
            network=network,
            base_url="",
            source="default",
            guard_triggered=False,
        )
        return ""
    network_l = network.strip().lower()
    if network_l in {"testnet", "testnet11"}:
        allow_mainnet = os.getenv("GREENFLOOR_ALLOW_MAINNET_COINSET_FOR_TESTNET11", "").strip()
        guard_triggered = (
            "coinset.org" in base
            and "testnet11.api.coinset.org" not in base
            and allow_mainnet != "1"
        )
        _debug_signing(
            "coinset_base_url_resolved",
            network=network,
            base_url=base,
            source="env",
            allow_mainnet_coinset_for_testnet11=allow_mainnet == "1",
            guard_triggered=guard_triggered,
        )
        if (
            "coinset.org" in base
            and "testnet11.api.coinset.org" not in base
            and allow_mainnet != "1"
        ):
            raise RuntimeError("coinset_base_url_mainnet_not_allowed_for_testnet11")
    else:
        _debug_signing(
            "coinset_base_url_resolved",
            network=network,
            base_url=base,
            source="env",
            guard_triggered=False,
        )
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
        _debug_signing(
            "cat_coin_discovery_start",
            network=network,
            receive_address=_redact_address(receive_address),
            asset_id=asset_id,
            cat_puzzle_hash=cat_puzzle_hash_hex,
        )
        records = coinset.get_coin_records_by_puzzle_hash(
            puzzle_hash_hex=cat_puzzle_hash_hex,
            include_spent_coins=False,
        )
        _debug_signing(
            "cat_coin_records_fetched",
            network=network,
            asset_id=asset_id,
            records_count=len(records),
        )
        if not records:
            return []

        clvm = sdk.Clvm()
        cats: list[Any] = []
        skipped_invalid_coin = 0
        skipped_missing_parent = 0
        skipped_invalid_parent_coin = 0
        skipped_unspent_parent = 0
        skipped_missing_parent_solution = 0
        skipped_missing_parent_solution_fields = 0
        skipped_parent_deserialize_error = 0
        skipped_parse_child_cats_empty = 0
        skipped_child_match_missing = 0
        for idx, record in enumerate(records):
            coin = _coin_from_record(sdk=sdk, record=record)
            if coin is None:
                skipped_invalid_coin += 1
                continue
            coin_id_hex = sdk.to_hex(coin.coin_id())
            parent_coin_id_hex = sdk.to_hex(coin.parent_coin_info)

            parent_record = coinset.get_coin_record_by_name(
                coin_name_hex=_to_coinset_hex(coin.parent_coin_info)
            )
            if parent_record is None:
                skipped_missing_parent += 1
                continue
            parent_coin = _coin_from_record(sdk=sdk, record=parent_record)
            if parent_coin is None:
                skipped_invalid_parent_coin += 1
                continue
            parent_spent_height = _spent_height_from_record(parent_record)
            if parent_spent_height <= 0:
                skipped_unspent_parent += 1
                continue

            _debug_signing(
                "cat_coin_record_parent_lookup",
                network=network,
                asset_id=asset_id,
                record_index=idx,
                coin_id=_redact_hex(coin_id_hex),
                parent_coin_id=_redact_hex(parent_coin_id_hex),
                coin_amount=int(coin.amount),
                parent_spent_height=parent_spent_height,
            )
            parent_solution_record = coinset.get_puzzle_and_solution(
                coin_id_hex=_to_coinset_hex(parent_coin.coin_id()),
                height=parent_spent_height,
            )
            if parent_solution_record is None:
                skipped_missing_parent_solution += 1
                continue
            puzzle_reveal_hex = str(parent_solution_record.get("puzzle_reveal", "")).strip()
            solution_hex = str(parent_solution_record.get("solution", "")).strip()
            if not puzzle_reveal_hex or not solution_hex:
                skipped_missing_parent_solution_fields += 1
                continue
            _debug_signing(
                "cat_coin_parent_solution_loaded",
                network=network,
                asset_id=asset_id,
                record_index=idx,
                coin_id=_redact_hex(coin_id_hex),
                parent_coin_id=_redact_hex(parent_coin_id_hex),
                puzzle_reveal_hex_len=len(puzzle_reveal_hex),
                solution_hex_len=len(solution_hex),
            )

            try:
                parent_coin_spend = _CoinSolutionView(
                    puzzle_reveal=_hex_to_bytes(puzzle_reveal_hex),
                    solution=_hex_to_bytes(solution_hex),
                )
                parent_puzzle_program = clvm.deserialize(parent_coin_spend.puzzle_reveal)
                parent_solution = clvm.deserialize(parent_coin_spend.solution)
                parent_puzzle = parent_puzzle_program.puzzle()
            except Exception:
                skipped_parent_deserialize_error += 1
                continue
            parse_mode = "non_empty"
            parse_error = ""
            try:
                parsed_children = parent_puzzle.parse_child_cats(parent_coin, parent_solution)
            except Exception as exc:
                parsed_children = None
                parse_mode = "exception"
                parse_error = f"{type(exc).__name__}:{exc}"
            parsed_children_count = len(parsed_children) if parsed_children else 0
            if parse_mode == "non_empty" and parsed_children_count == 0:
                parse_mode = "empty"
            _debug_signing(
                "cat_coin_parse_child_cats_result",
                network=network,
                asset_id=asset_id,
                record_index=idx,
                coin_id=_redact_hex(coin_id_hex),
                parent_coin_id=_redact_hex(parent_coin_id_hex),
                parsed_children_count=parsed_children_count,
                parse_mode=parse_mode,
                parse_error=parse_error,
            )
            if parse_mode == "exception":
                _debug_signing(
                    "cat_coin_parse_child_cats_error",
                    network=network,
                    asset_id=asset_id,
                    record_index=idx,
                    coin_id=_redact_hex(coin_id_hex),
                    parent_coin_id=_redact_hex(parent_coin_id_hex),
                    parse_error=parse_error,
                )
            _debug_probe_parent_create_coin_outputs(
                sdk=sdk,
                parent_coin_spend=parent_coin_spend,
                parent_coin=parent_coin,
                child_coin=coin,
                network=network,
                asset_id=asset_id,
                record_index=idx,
            )
            if not parsed_children:
                _capture_cat_parse_case(
                    {
                        "case_id": f"{network}-{asset_id[:12]}-{coin_id_hex[:12]}-idx{idx}",
                        "capture_reason": f"parse_child_cats_{parse_mode}",
                        "network": network,
                        "asset_id": asset_id,
                        "record_index": idx,
                        "coin_id": coin_id_hex,
                        "coin_parent_coin_id": parent_coin_id_hex,
                        "coin_amount": int(coin.amount),
                        "coin_puzzle_hash": sdk.to_hex(coin.puzzle_hash),
                        "parent_coin_id": sdk.to_hex(parent_coin.coin_id()),
                        "parent_coin_parent_coin_id": sdk.to_hex(parent_coin.parent_coin_info),
                        "parent_coin_amount": int(parent_coin.amount),
                        "parent_coin_puzzle_hash": sdk.to_hex(parent_coin.puzzle_hash),
                        "parent_spent_height": parent_spent_height,
                        "puzzle_reveal": puzzle_reveal_hex,
                        "solution": solution_hex,
                        "parse_mode": parse_mode,
                        "parse_error": parse_error,
                    }
                )
                skipped_parse_child_cats_empty += 1
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
                skipped_child_match_missing += 1
        _debug_signing(
            "cat_coin_discovery_result",
            network=network,
            asset_id=asset_id,
            records_count=len(records),
            discovered_cats=len(cats),
            skipped_invalid_coin=skipped_invalid_coin,
            skipped_missing_parent=skipped_missing_parent,
            skipped_invalid_parent_coin=skipped_invalid_parent_coin,
            skipped_unspent_parent=skipped_unspent_parent,
            skipped_missing_parent_solution=skipped_missing_parent_solution,
            skipped_missing_parent_solution_fields=skipped_missing_parent_solution_fields,
            skipped_parent_deserialize_error=skipped_parent_deserialize_error,
            skipped_parse_child_cats_empty=skipped_parse_child_cats_empty,
            skipped_child_match_missing=skipped_child_match_missing,
        )
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
        _debug_signing(
            "offer_coin_selection_mode",
            mode="xch",
            network=network,
            receive_address=_redact_address(receive_address),
            offer_asset_id=offer_asset_id,
            offer_amount=offer_amount,
            request_asset_id=request_asset_id,
            request_amount=request_amount,
        )
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
        _debug_signing(
            "offer_coin_selection_mode",
            mode="cat",
            network=network,
            receive_address=_redact_address(receive_address),
            offer_asset_id=offer_asset_id,
            offer_amount=offer_amount,
            request_asset_id=request_asset_id,
            request_amount=request_amount,
        )
        try:
            cat_coins = _list_unspent_cat_coins(
                sdk=sdk,
                receive_address=receive_address,
                network=network,
                asset_id=offer_asset,
            )
        except Exception as exc:
            return None, str(exc)
        _debug_signing(
            "offer_cat_coins_discovered",
            network=network,
            offer_asset_id=offer_asset_id,
            discovered_count=len(cat_coins),
            discovered_total_amount=sum(int(c.coin.amount) for c in cat_coins),
        )
        if not cat_coins:
            return None, "no_unspent_offer_cat_coins"
        offered_selected_cats = _select_cats(cat_coins, offer_amount)
        _debug_signing(
            "offer_cat_coins_selected",
            network=network,
            offer_asset_id=offer_asset_id,
            selected_count=len(offered_selected_cats),
            selected_total_amount=sum(int(c.coin.amount) for c in offered_selected_cats),
            target_offer_amount=offer_amount,
        )
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
        requested_payment_memos = clvm.list([clvm.atom(receive_puzzle_hash)])
        requested_payment = sdk.Payment(
            receive_puzzle_hash, request_amount, requested_payment_memos
        )
        notarized_payment = sdk.NotarizedPayment(offer_nonce, [requested_payment])
        _debug_signing(
            "offer_primary_path_input_actions_built",
            network=network,
            offer_asset_id=offer_asset_id,
            request_asset_id=request_asset_id,
            dry_run=bool(dry_run),
            offer_amount=offer_amount,
            request_amount=request_amount,
            offered_total=offered_total,
            settlement_puzzle_hash=_redact_hex(sdk.to_hex(settlement_puzzle_hash)),
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
                pending_cat_spends.append(sdk.CatSpend(pending_cat, delegated))
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
        sample_coin_spends: list[dict[str, Any]] = []
        for idx, coin_spend in enumerate(coin_spends):
            coin = coin_spend.coin
            parent_hex = sdk.to_hex(coin.parent_coin_info)
            puzzle_hash_hex = sdk.to_hex(coin.puzzle_hash)
            if idx < 6:
                sample_coin_spends.append(
                    {
                        "coin_id": _redact_hex(sdk.to_hex(coin.coin_id())),
                        "parent_coin_id": _redact_hex(parent_hex),
                        "puzzle_hash": _redact_hex(puzzle_hash_hex),
                        "amount": int(coin.amount),
                    }
                )
        _debug_signing(
            "offer_spend_bundle_shape",
            network=network,
            offer_asset_id=offer_asset_id,
            request_asset_id=request_asset_id,
            dry_run=bool(dry_run),
            coin_spends_count=len(coin_spends),
            sample_coin_spends=sample_coin_spends,
        )
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
            _debug_signing(
                "offer_no_agg_sig_targets_using_identity_signature",
                network=network,
                offer_asset_id=offer_asset_id,
                request_asset_id=request_asset_id,
                dry_run=bool(dry_run),
                coin_spends_count=len(coin_spends),
            )
            aggregate_sig = sdk.Signature.aggregate([])
        else:
            aggregate_sig = sdk.Signature.aggregate(signatures)
        input_spend_bundle = sdk.SpendBundle(coin_spends, aggregate_sig)
        spend_bundle = sdk.from_input_spend_bundle(input_spend_bundle, [notarized_payment])
        if _is_signing_debug_enabled():
            parse_probe_error = ""
            parse_probe_ok = False
            offered_settlement_cats_count: int | None = None
            try:
                probe_clvm = sdk.Clvm()
                offer_asset = offer_asset_id.strip().lower()
                if offer_asset not in {"", "xch", "txch", "1"}:
                    offered_settlement_cats_count = len(
                        probe_clvm.offer_settlement_cats(spend_bundle, _hex_to_bytes(offer_asset))
                    )
                # If this returns without exception, Offer::from_spend_bundle parsed successfully.
                parse_probe_ok = True
            except Exception as exc:
                parse_probe_error = f"{type(exc).__name__}:{exc}"
            _debug_signing(
                "offer_sdk_parse_probe",
                network=network,
                offer_asset_id=offer_asset_id,
                request_asset_id=request_asset_id,
                dry_run=bool(dry_run),
                parse_probe_ok=parse_probe_ok,
                parse_probe_error=parse_probe_error,
                offered_settlement_cats_count=offered_settlement_cats_count,
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

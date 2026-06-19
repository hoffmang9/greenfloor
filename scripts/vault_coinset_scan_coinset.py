"""Vault-specific Coinset scan helpers (CAT lineage detection)."""

from __future__ import annotations

from typing import Any

from greenfloor_scripts.chia_sdk_helpers import hex_to_bytes, safe_int, to_coinset_hex
from greenfloor_scripts.coinset_scanner import CoinsetScanner
from greenfloor_scripts.hex_subprocess import normalize_hex_id


def _coin_from_record(*, sdk: Any, record: dict[str, Any]) -> Any | None:
    coin_data = record.get("coin")
    if not isinstance(coin_data, dict):
        return None
    parent_hex = normalize_hex_id(coin_data.get("parent_coin_info"))
    puzzle_hex = normalize_hex_id(coin_data.get("puzzle_hash"))
    if not parent_hex or not puzzle_hex:
        return None
    try:
        return sdk.Coin(
            hex_to_bytes(parent_hex),
            hex_to_bytes(puzzle_hex),
            int(coin_data.get("amount", 0)),
        )
    except Exception:
        return None


def _detect_cat_asset_id(
    *,
    sdk: Any,
    coinset: CoinsetScanner,
    coin_id: str,
    record: dict[str, Any],
    cat_asset_cache: dict[str, str],
    parent_record_cache: dict[str, dict[str, Any] | None],
    puzzle_solution_cache: dict[str, dict[str, Any] | None],
    parent_lineage_cache: dict[str, dict[str, Any]],
) -> str | None:
    cached = cat_asset_cache.get(coin_id)
    if cached is not None:
        return cached or None
    coin = _coin_from_record(sdk=sdk, record=record)
    if coin is None:
        cat_asset_cache[coin_id] = ""
        return None
    parent_coin_id_hex = normalize_hex_id(coin.parent_coin_info.hex()) or ""
    if not parent_coin_id_hex:
        cat_asset_cache[coin_id] = ""
        return None
    parent_lineage = parent_lineage_cache.get(parent_coin_id_hex)
    if isinstance(parent_lineage, dict):
        cached_child_assets = parent_lineage.get("child_asset_ids")
        if isinstance(cached_child_assets, dict):
            cached_asset = normalize_hex_id(cached_child_assets.get(coin_id))
            if cached_asset:
                cat_asset_cache[coin_id] = cached_asset
                return cached_asset
            if coin_id in cached_child_assets:
                cat_asset_cache[coin_id] = ""
                return None

    parent_record = parent_record_cache.get(parent_coin_id_hex)
    if parent_record is None and parent_coin_id_hex not in parent_record_cache:
        rows = coinset.by_names(
            coin_names=[to_coinset_hex(coin.parent_coin_info)],
            include_spent=True,
        )
        parent_record = rows[0] if rows else None
        parent_record_cache[parent_coin_id_hex] = parent_record
    if not isinstance(parent_record, dict):
        cat_asset_cache[coin_id] = ""
        return None
    parent_coin = _coin_from_record(sdk=sdk, record=parent_record)
    if parent_coin is None:
        cat_asset_cache[coin_id] = ""
        return None
    spent_height = safe_int(parent_record.get("spent_block_index"), default=0)
    if spent_height <= 0:
        cat_asset_cache[coin_id] = ""
        return None

    parent_coin_name = normalize_hex_id(sdk.to_hex(parent_coin.coin_id())) or ""
    if not parent_coin_name:
        cat_asset_cache[coin_id] = ""
        return None
    solution_cache_key = f"{parent_coin_name}:{spent_height}"
    solution = puzzle_solution_cache.get(solution_cache_key)
    if solution is None and solution_cache_key not in puzzle_solution_cache:
        solution = coinset.puzzle_and_solution(
            coin_id_hex=to_coinset_hex(parent_coin.coin_id()),
            height=spent_height,
        )
        puzzle_solution_cache[solution_cache_key] = solution
    if not isinstance(solution, dict):
        cat_asset_cache[coin_id] = ""
        return None
    puzzle_reveal_hex = str(solution.get("puzzle_reveal", "")).strip()
    solution_hex = str(solution.get("solution", "")).strip()
    if not puzzle_reveal_hex or not solution_hex:
        cat_asset_cache[coin_id] = ""
        return None
    try:
        clvm = sdk.Clvm()
        parent_puzzle_program = clvm.deserialize(hex_to_bytes(puzzle_reveal_hex))
        parent_solution_program = clvm.deserialize(hex_to_bytes(solution_hex))
        parsed_children = parent_puzzle_program.puzzle().parse_child_cats(
            parent_coin, parent_solution_program
        )
    except Exception:
        cat_asset_cache[coin_id] = ""
        return None
    if not parsed_children:
        parent_lineage_cache[parent_coin_id_hex] = {
            "spent_height": spent_height,
            "child_asset_ids": {coin_id: ""},
        }
        cat_asset_cache[coin_id] = ""
        return None
    wanted_id = sdk.to_hex(coin.coin_id())
    child_assets: dict[str, str] = {}
    for cat in parsed_children:
        child_coin = getattr(cat, "coin", None)
        info = getattr(cat, "info", None)
        if child_coin is None or info is None:
            continue
        child_id = normalize_hex_id(sdk.to_hex(child_coin.coin_id())) or ""
        if not child_id:
            continue
        asset_id = normalize_hex_id(sdk.to_hex(info.asset_id)) or ""
        child_assets[child_id] = asset_id
        cat_asset_cache[child_id] = asset_id

    if coin_id not in child_assets:
        child_assets[coin_id] = ""
    parent_lineage_cache[parent_coin_id_hex] = {
        "spent_height": spent_height,
        "child_asset_ids": child_assets,
    }
    target_asset = child_assets.get(wanted_id) or child_assets.get(coin_id) or ""
    if target_asset:
        cat_asset_cache[coin_id] = target_asset
        return target_asset
    cat_asset_cache[coin_id] = ""
    return None

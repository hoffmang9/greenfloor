"""Checkpoint serialization for vault Coinset scans."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from greenfloor_scripts.chia_sdk_helpers import safe_int
from greenfloor_scripts.hex_subprocess import normalize_hex_id


@dataclass(slots=True)
class CoinRow:
    coin_id: str
    puzzle_hash: str
    parent_coin_info: str
    amount: int
    confirmed_block_index: int
    spent_block_index: int
    discovered_nonces: list[int]
    discovered_by_puzzle_hash: bool
    discovered_by_hint: bool
    coin_type: str
    cat_asset_id: str | None
    cat_symbols: list[str]


def _clear_cache_files(paths: list[str]) -> dict[str, str]:
    results: dict[str, str] = {}
    for raw_path in paths:
        clean = str(raw_path).strip()
        if not clean:
            continue
        path = Path(clean).expanduser()
        key = str(path)
        if path.exists():
            try:
                path.unlink()
                results[key] = "deleted"
            except Exception as exc:  # noqa: BLE001
                results[key] = f"delete_failed:{exc}"
        else:
            results[key] = "not_found"
    return results


def _coin_row_to_dict(row: CoinRow) -> dict[str, Any]:
    return {
        "coin_id": row.coin_id,
        "puzzle_hash": row.puzzle_hash,
        "parent_coin_info": row.parent_coin_info,
        "amount": int(row.amount),
        "confirmed_block_index": int(row.confirmed_block_index),
        "spent_block_index": int(row.spent_block_index),
        "discovered_nonces": sorted(int(nonce) for nonce in row.discovered_nonces),
        "discovered_by_puzzle_hash": bool(row.discovered_by_puzzle_hash),
        "discovered_by_hint": bool(row.discovered_by_hint),
        "coin_type": str(row.coin_type),
        "cat_asset_id": normalize_hex_id(row.cat_asset_id) if row.cat_asset_id else None,
        "cat_symbols": [str(symbol) for symbol in row.cat_symbols],
    }


def _coin_row_from_dict(payload: dict[str, Any]) -> CoinRow | None:
    coin_id = normalize_hex_id(payload.get("coin_id"))
    if not coin_id:
        return None
    nonces_raw = payload.get("discovered_nonces")
    nonces = [int(value) for value in nonces_raw] if isinstance(nonces_raw, list) else []
    cat_symbols_raw = payload.get("cat_symbols")
    cat_symbols = [
        str(symbol).strip()
        for symbol in (cat_symbols_raw if isinstance(cat_symbols_raw, list) else [])
        if str(symbol).strip()
    ]
    return CoinRow(
        coin_id=coin_id,
        puzzle_hash=normalize_hex_id(payload.get("puzzle_hash")) or "",
        parent_coin_info=normalize_hex_id(payload.get("parent_coin_info")) or "",
        amount=safe_int(payload.get("amount"), default=0),
        confirmed_block_index=safe_int(payload.get("confirmed_block_index"), default=0),
        spent_block_index=safe_int(payload.get("spent_block_index"), default=0),
        discovered_nonces=sorted(set(nonces)),
        discovered_by_puzzle_hash=bool(payload.get("discovered_by_puzzle_hash", False)),
        discovered_by_hint=bool(payload.get("discovered_by_hint", False)),
        coin_type=str(payload.get("coin_type", "UNKNOWN")).strip().upper() or "UNKNOWN",
        cat_asset_id=normalize_hex_id(payload.get("cat_asset_id")) or None,
        cat_symbols=cat_symbols,
    )


def _load_scan_checkpoint(
    *,
    checkpoint_file: str,
    network: str,
    launcher_id: str,
    include_spent: bool,
) -> tuple[
    int, dict[int, str], dict[str, CoinRow], dict[str, str], dict[str, dict[str, Any]], int | None
]:
    path = Path(checkpoint_file).expanduser()
    if not path.exists():
        return 0, {}, {}, {}, {}, None
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return 0, {}, {}, {}, {}, None
    if not isinstance(parsed, dict):
        return 0, {}, {}, {}, {}, None
    if normalize_hex_id(parsed.get("launcher_id")) != normalize_hex_id(launcher_id):
        return 0, {}, {}, {}, {}, None
    if str(parsed.get("network", "")).strip().lower() != str(network).strip().lower():
        return 0, {}, {}, {}, {}, None
    if bool(parsed.get("include_spent", False)) != bool(include_spent):
        return 0, {}, {}, {}, {}, None

    raw_nonce_map = parsed.get("nonce_to_p2")
    nonce_to_p2: dict[int, str] = {}
    if isinstance(raw_nonce_map, dict):
        for nonce_key, p2_hash in raw_nonce_map.items():
            try:
                nonce = int(nonce_key)
            except (TypeError, ValueError):
                continue
            clean_hash = normalize_hex_id(p2_hash)
            if clean_hash:
                nonce_to_p2[nonce] = clean_hash

    raw_rows = parsed.get("coin_rows")
    by_coin_id: dict[str, CoinRow] = {}
    if isinstance(raw_rows, list):
        for row_raw in raw_rows:
            if not isinstance(row_raw, dict):
                continue
            row = _coin_row_from_dict(row_raw)
            if row is None:
                continue
            by_coin_id[row.coin_id] = row

    raw_cat_cache = parsed.get("cat_asset_cache")
    cat_asset_cache: dict[str, str] = {}
    if isinstance(raw_cat_cache, dict):
        for coin_id_raw, asset_id_raw in raw_cat_cache.items():
            coin_id = normalize_hex_id(coin_id_raw)
            if not coin_id:
                continue
            asset_id = normalize_hex_id(asset_id_raw) or ""
            cat_asset_cache[coin_id] = asset_id

    raw_parent_lineage = parsed.get("parent_lineage_cache")
    parent_lineage_cache: dict[str, dict[str, Any]] = {}
    if isinstance(raw_parent_lineage, dict):
        for parent_id_raw, lineage_raw in raw_parent_lineage.items():
            parent_id = normalize_hex_id(parent_id_raw)
            if not parent_id or not isinstance(lineage_raw, dict):
                continue
            child_assets_raw = lineage_raw.get("child_asset_ids")
            child_assets: dict[str, str] = {}
            if isinstance(child_assets_raw, dict):
                for child_id_raw, asset_id_raw in child_assets_raw.items():
                    child_id = normalize_hex_id(child_id_raw)
                    if not child_id:
                        continue
                    child_assets[child_id] = normalize_hex_id(asset_id_raw) or ""
            parent_lineage_cache[parent_id] = {
                "spent_height": safe_int(lineage_raw.get("spent_height"), default=0),
                "child_asset_ids": child_assets,
            }

    max_nonce_completed = safe_int(parsed.get("max_nonce_completed"), default=-1)
    last_synced_height_raw = parsed.get("last_synced_height")
    last_synced_height = (
        safe_int(last_synced_height_raw, default=-1) if last_synced_height_raw is not None else -1
    )
    if last_synced_height < 0:
        last_synced_height = None
    next_nonce = max(0, max_nonce_completed + 1)
    return (
        next_nonce,
        nonce_to_p2,
        by_coin_id,
        cat_asset_cache,
        parent_lineage_cache,
        last_synced_height,
    )


def _child_asset_id_items(lineage: dict[str, Any]) -> list[tuple[str, Any]]:
    child_asset_ids = lineage.get("child_asset_ids")
    if isinstance(child_asset_ids, dict):
        return list(child_asset_ids.items())
    return []


def _save_scan_checkpoint(
    *,
    checkpoint_file: str,
    network: str,
    launcher_id: str,
    include_spent: bool,
    max_nonce_completed: int,
    nonce_to_p2: dict[int, str],
    by_coin_id: dict[str, CoinRow],
    cat_asset_cache: dict[str, str],
    parent_lineage_cache: dict[str, dict[str, Any]],
    last_synced_height: int | None,
    scan_start_height: int | None,
    scan_end_height: int | None,
) -> None:
    path = Path(checkpoint_file).expanduser()
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "version": 1,
        "network": str(network).strip().lower(),
        "launcher_id": normalize_hex_id(launcher_id) or "",
        "include_spent": bool(include_spent),
        "max_nonce_completed": int(max_nonce_completed),
        "last_synced_height": int(last_synced_height) if last_synced_height is not None else None,
        "scan_window": {
            "start_height": int(scan_start_height) if scan_start_height is not None else None,
            "end_height": int(scan_end_height) if scan_end_height is not None else None,
        },
        "nonce_to_p2": {str(k): v for k, v in sorted(nonce_to_p2.items())},
        "coin_rows": [
            _coin_row_to_dict(row) for row in sorted(by_coin_id.values(), key=lambda r: r.coin_id)
        ],
        "cat_asset_cache": {
            coin_id: asset_id for coin_id, asset_id in sorted(cat_asset_cache.items())
        },
        "parent_lineage_cache": {
            parent_id: {
                "spent_height": safe_int(lineage.get("spent_height"), default=0),
                "child_asset_ids": {
                    child_id: normalize_hex_id(asset_id) or ""
                    for child_id, asset_id in sorted(
                        (
                            (normalize_hex_id(raw_child_id) or "", raw_asset_id)
                            for raw_child_id, raw_asset_id in _child_asset_id_items(lineage)
                        ),
                        key=lambda item: item[0],
                    )
                    if child_id
                },
            }
            for parent_id, lineage in sorted(parent_lineage_cache.items())
        },
    }
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")

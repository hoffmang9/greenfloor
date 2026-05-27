"""CAT catalog file I/O and Dexie metadata helpers."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from greenfloor import asset_label_catalog
from greenfloor.config.io import load_yaml


def coerce_optional_str(raw: object) -> str | None:
    value = str(raw or "").strip()
    if not value:
        return None
    return value


def try_parse_optional_float(raw: str | None) -> float | None:
    if raw is None:
        return None
    cleaned = str(raw).strip()
    if not cleaned:
        return None
    return float(cleaned)


def load_cats_catalog(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {"cats": []}
    payload = load_yaml(path)
    rows = payload.get("cats")
    if rows is None:
        payload["cats"] = []
        return payload
    if not isinstance(rows, list):
        raise ValueError("cats config must contain cats as a list")
    return payload


def derive_cat_metadata_from_dexie_row(row: dict[str, Any]) -> dict[str, Any]:
    cat_id_candidates = [
        row.get("id"),
        row.get("assetId"),
        row.get("asset_id"),
        row.get("tokenId"),
        row.get("token_id"),
        row.get("base_currency"),
    ]
    cat_id = ""
    for candidate in cat_id_candidates:
        normalized = asset_label_catalog._normalize_hex_asset_id(str(candidate or ""))
        if asset_label_catalog._is_hex_asset_id(normalized):
            cat_id = normalized
            break
    symbol = coerce_optional_str(row.get("code") or row.get("base_code") or row.get("symbol")) or ""
    name = (
        coerce_optional_str(
            row.get("name")
            or row.get("base_name")
            or row.get("display_name")
            or row.get("displayName")
        )
        or ""
    )
    dexie_ticker_id = coerce_optional_str(row.get("ticker_id") or row.get("tickerId"))
    dexie_pool_id = coerce_optional_str(row.get("pool_id") or row.get("poolId"))
    dexie_last_price = coerce_optional_str(
        row.get("last_price_xch") or row.get("lastPriceXch") or row.get("price_xch")
    )
    return {
        "asset_id": cat_id,
        "base_symbol": symbol,
        "name": name,
        "dexie": {
            "ticker_id": dexie_ticker_id,
            "pool_id": dexie_pool_id,
            "last_price_xch": dexie_last_price,
        },
    }

"""CLI commands for managing the local CAT catalog."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from greenfloor import asset_label_catalog
from greenfloor.cli.cats_catalog import (
    coerce_optional_str,
    derive_cat_metadata_from_dexie_row,
    load_cats_catalog,
    try_parse_optional_float,
)
from greenfloor.cli.prompts import prompt_yes_no
from greenfloor.config.io import write_yaml
from greenfloor.runtime.json_output import format_json_output


def cats_add(
    *,
    cats_path: Path,
    network: str,
    cat_id: str | None,
    ticker: str | None,
    name: str | None,
    base_symbol: str | None,
    ticker_id: str | None,
    pool_id: str | None,
    last_price_xch: str | None,
    target_usd_per_unit: str | None,
    use_dexie_lookup: bool,
    replace: bool,
) -> int:
    ref_cat_id = asset_label_catalog._normalize_hex_asset_id(str(cat_id or ""))
    ref_ticker = str(ticker or "").strip()
    if not ref_cat_id and not ref_ticker:
        print(format_json_output({"added": False, "error": "must_provide_cat_id_or_ticker"}))
        return 2

    dexie_row: dict[str, Any] | None = None
    if use_dexie_lookup:
        if ref_cat_id:
            dexie_row = asset_label_catalog._dexie_lookup_token_for_cat_id(
                canonical_cat_id_hex=ref_cat_id,
                network=network,
            )
        if dexie_row is None and ref_ticker:
            dexie_row = asset_label_catalog._dexie_lookup_token_for_symbol(
                asset_ref=ref_ticker, network=network
            )
        if dexie_row is not None:
            inferred = derive_cat_metadata_from_dexie_row(dexie_row)
            inferred_cat_id = asset_label_catalog._normalize_hex_asset_id(
                str(inferred.get("asset_id", ""))
            )
            if inferred_cat_id and asset_label_catalog._is_hex_asset_id(inferred_cat_id):
                enriched = asset_label_catalog._dexie_lookup_token_for_cat_id(
                    canonical_cat_id_hex=inferred_cat_id,
                    network=network,
                )
                if enriched is not None:
                    dexie_row = dict(enriched)
                    if "code" not in dexie_row:
                        dexie_row["code"] = inferred.get("base_symbol")
                    if "name" not in dexie_row:
                        dexie_row["name"] = inferred.get("name")
                    if "id" not in dexie_row:
                        dexie_row["id"] = inferred_cat_id

    dexie_meta = derive_cat_metadata_from_dexie_row(dexie_row or {})
    resolved_asset_id = ref_cat_id or asset_label_catalog._normalize_hex_asset_id(
        str(dexie_meta.get("asset_id", ""))
    )
    if not asset_label_catalog._is_hex_asset_id(resolved_asset_id):
        print(format_json_output({"added": False, "error": "cat_id_required_and_must_be_64_hex"}))
        return 2

    resolved_symbol = (
        str(base_symbol or "").strip()
        or str(dexie_meta.get("base_symbol", "")).strip()
        or ref_ticker.strip().upper()
    )
    if not resolved_symbol:
        print(format_json_output({"added": False, "error": "base_symbol_is_required"}))
        return 2
    resolved_name = str(name or "").strip() or str(dexie_meta.get("name", "")).strip()
    if not resolved_name:
        resolved_name = resolved_symbol

    resolved_ticker_id = coerce_optional_str(ticker_id) or coerce_optional_str(
        (dexie_meta.get("dexie") or {}).get("ticker_id")
    )
    resolved_pool_id = coerce_optional_str(pool_id) or coerce_optional_str(
        (dexie_meta.get("dexie") or {}).get("pool_id")
    )
    resolved_last_price_xch = coerce_optional_str(last_price_xch) or coerce_optional_str(
        (dexie_meta.get("dexie") or {}).get("last_price_xch")
    )

    parsed_target_usd_per_unit: float | None
    try:
        parsed_target_usd_per_unit = try_parse_optional_float(target_usd_per_unit)
    except ValueError:
        print(
            format_json_output(
                {
                    "added": False,
                    "error": "target_usd_per_unit_must_be_numeric_if_provided",
                }
            )
        )
        return 2

    cats_payload = load_cats_catalog(cats_path)
    rows = cats_payload.get("cats")
    if not isinstance(rows, list):
        raise ValueError("cats config must contain cats as a list")

    new_entry: dict[str, Any] = {
        "name": resolved_name,
        "base_symbol": resolved_symbol,
        "asset_id": resolved_asset_id,
        "target_usd_per_unit": parsed_target_usd_per_unit,
        "dexie": {
            "ticker_id": resolved_ticker_id,
            "pool_id": resolved_pool_id,
            "last_price_xch": resolved_last_price_xch,
        },
    }
    existing_index = next(
        (
            idx
            for idx, row in enumerate(rows)
            if isinstance(row, dict)
            and asset_label_catalog._normalize_hex_asset_id(str(row.get("asset_id", "")))
            == resolved_asset_id
        ),
        None,
    )
    if existing_index is not None and not replace:
        print(
            format_json_output(
                {
                    "added": False,
                    "error": "cat_already_exists_use_replace",
                    "asset_id": resolved_asset_id,
                }
            )
        )
        return 2
    if existing_index is None:
        rows.append(new_entry)
    else:
        rows[existing_index] = new_entry

    rows.sort(
        key=lambda row: (
            str(row.get("base_symbol", "")).lower() if isinstance(row, dict) else "",
            str(row.get("asset_id", "")).lower() if isinstance(row, dict) else "",
        )
    )
    cats_payload["cats"] = rows
    write_yaml(cats_path, cats_payload)

    print(
        format_json_output(
            {
                "added": True,
                "replaced_existing": existing_index is not None,
                "cats_config": str(cats_path),
                "cat": new_entry,
                "dexie_lookup_used": bool(use_dexie_lookup),
                "dexie_match_found": dexie_row is not None,
            }
        )
    )
    return 0


def cats_list(*, cats_path: Path) -> int:
    cats_payload = load_cats_catalog(cats_path)
    rows = cats_payload.get("cats")
    if not isinstance(rows, list):
        raise ValueError("cats config must contain cats as a list")
    print(
        format_json_output(
            {
                "cats_config": str(cats_path),
                "count": len(rows),
                "cats": rows,
            }
        )
    )
    return 0


def cats_delete(
    *,
    cats_path: Path,
    network: str,
    cat_id: str | None,
    ticker: str | None,
    use_dexie_lookup: bool,
    confirm_delete: bool,
    preflight_only: bool,
) -> int:
    ref_cat_id = asset_label_catalog._normalize_hex_asset_id(str(cat_id or ""))
    ref_ticker = str(ticker or "").strip()
    if not ref_cat_id and not ref_ticker:
        print(format_json_output({"deleted": False, "error": "must_provide_cat_id_or_ticker"}))
        return 2

    cats_payload = load_cats_catalog(cats_path)
    rows = cats_payload.get("cats")
    if not isinstance(rows, list):
        raise ValueError("cats config must contain cats as a list")

    target_asset_id = ref_cat_id
    if not target_asset_id:
        ticker_matches: list[dict[str, Any]] = []
        normalized_ticker = ref_ticker.lower()
        for row in rows:
            if not isinstance(row, dict):
                continue
            row_symbol = str(row.get("base_symbol", "")).strip().lower()
            row_name = str(row.get("name", "")).strip().lower()
            if normalized_ticker and (
                row_symbol == normalized_ticker or row_name == normalized_ticker
            ):
                ticker_matches.append(row)
        if len(ticker_matches) == 1:
            target_asset_id = asset_label_catalog._normalize_hex_asset_id(
                str(ticker_matches[0].get("asset_id", ""))
            )
        elif len(ticker_matches) > 1:
            print(
                format_json_output(
                    {
                        "deleted": False,
                        "error": "ambiguous_ticker_matches_multiple_cats",
                        "ticker": ref_ticker,
                    }
                )
            )
            return 2

    if not target_asset_id and use_dexie_lookup and ref_ticker:
        dexie_row = asset_label_catalog._dexie_lookup_token_for_symbol(
            asset_ref=ref_ticker, network=network
        )
        if dexie_row is not None:
            inferred = derive_cat_metadata_from_dexie_row(dexie_row)
            target_asset_id = asset_label_catalog._normalize_hex_asset_id(
                str(inferred.get("asset_id", ""))
            )

    if not asset_label_catalog._is_hex_asset_id(target_asset_id):
        print(format_json_output({"deleted": False, "error": "cat_id_required_and_must_be_64_hex"}))
        return 2

    delete_index = next(
        (
            idx
            for idx, row in enumerate(rows)
            if isinstance(row, dict)
            and asset_label_catalog._normalize_hex_asset_id(str(row.get("asset_id", "")))
            == target_asset_id
        ),
        None,
    )
    if delete_index is None:
        print(
            format_json_output(
                {
                    "deleted": False,
                    "error": "cat_not_found",
                    "asset_id": target_asset_id,
                }
            )
        )
        return 2

    candidate_entry = rows[delete_index]
    preflight_payload = {
        "preflight": True,
        "delete_requested": True,
        "cats_config": str(cats_path),
        "cat": candidate_entry,
    }
    print(format_json_output(preflight_payload))
    if preflight_only:
        print(
            format_json_output(
                {
                    "deleted": False,
                    "preflight_only": True,
                    "cats_config": str(cats_path),
                    "cat": candidate_entry,
                }
            )
        )
        return 0

    if not confirm_delete:
        confirmation_message = (
            f"Delete CAT {str(candidate_entry.get('base_symbol', '')).strip() or target_asset_id} "
            f"({target_asset_id}) from {cats_path}?"
        )
        if not prompt_yes_no(confirmation_message, prompt_for_override=None):
            print(
                format_json_output(
                    {
                        "deleted": False,
                        "error": "delete_not_confirmed",
                        "cats_config": str(cats_path),
                        "cat": candidate_entry,
                    }
                )
            )
            return 2

    deleted_entry = rows.pop(delete_index)
    cats_payload["cats"] = rows
    write_yaml(cats_path, cats_payload)
    print(
        format_json_output(
            {
                "deleted": True,
                "cats_config": str(cats_path),
                "cat": deleted_entry,
            }
        )
    )
    return 0

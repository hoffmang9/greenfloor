"""Thin wrapper around the Rust engine vault path (PyO3 module ``greenfloor_engine``)."""

from __future__ import annotations

from typing import Any

from greenfloor.core.engine_bridge import import_engine


def resolve_vault_context(program_path: str) -> dict[str, Any]:
    """Load vault display context from program config via the Rust engine."""
    engine = import_engine()
    result = engine.resolve_vault_context(str(program_path))
    if not isinstance(result, dict):
        raise TypeError("resolve_vault_context returned non-dict result")
    return result


def build_mixed_split(program_path: str, request_dict: dict[str, Any]) -> dict[str, Any]:
    """Build (and optionally broadcast) a vault CAT mixed split via the Rust engine."""
    engine = import_engine()
    result = engine.build_mixed_split(str(program_path), request_dict)
    if not isinstance(result, dict):
        raise TypeError("build_mixed_split returned non-dict result")
    return result


def vault_mixed_split_request_from_payload(payload: dict[str, Any]) -> dict[str, Any]:
    raw_outputs = payload.get("output_amounts_base_units", [])
    output_amounts: list[int] = []
    if isinstance(raw_outputs, list):
        for value in raw_outputs:
            output_amounts.append(int(value))
    raw_coin_ids = payload.get("selected_coin_ids", [])
    coin_ids: list[str] = []
    if isinstance(raw_coin_ids, list):
        for value in raw_coin_ids:
            clean = str(value).strip().lower()
            if clean.startswith("0x"):
                clean = clean[2:]
            if clean:
                coin_ids.append(clean)
    return {
        "receive_address": str(payload.get("receive_address", "")).strip(),
        "asset_id": str(payload.get("asset_id", "")).strip().lower(),
        "output_amounts": output_amounts,
        "coin_ids": coin_ids,
        "allow_sub_cat_output": bool(payload.get("allow_sub_cat_output", False)),
        "fee_mojos": int(payload.get("fee_mojos", 0)),
    }

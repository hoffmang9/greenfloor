"""Shared chia-wallet-sdk helpers for vault scan scripts."""

from __future__ import annotations

import importlib
import json
from typing import Any

from greenfloor_scripts.engine_subprocess import run_engine_json


def import_sdk() -> Any:
    return importlib.import_module("chia_wallet_sdk")


def hex_to_bytes(value: str) -> bytes:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if len(raw) % 2:
        raw = f"0{raw}"
    return bytes.fromhex(raw)


def to_coinset_hex(value: bytes) -> str:
    return f"0x{value.hex()}"


def safe_int(value: object, default: int = 0) -> int:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return default


def coin_id_from_record(record: dict[str, Any]) -> str:
    try:
        payload = run_engine_json(
            [
                "coinset",
                "coin-id-from-record",
                "--record-json",
                json.dumps(record, separators=(",", ":")),
            ]
        )
    except Exception:
        return ""
    if not isinstance(payload, dict):
        return ""
    coin_id = payload.get("coin_id")
    if not isinstance(coin_id, str):
        return ""
    return coin_id

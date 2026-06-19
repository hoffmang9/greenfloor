"""Shared chia-wallet-sdk helpers for vault scan scripts."""

from __future__ import annotations

import importlib
from typing import Any

from greenfloor_scripts.hex_subprocess import normalize_hex_id


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
    coin = record.get("coin")
    if not isinstance(coin, dict):
        return ""
    for candidate in (
        coin.get("name"),
        coin.get("coin_id"),
        coin.get("coin_name"),
        record.get("name"),
    ):
        normalized = normalize_hex_id(candidate)
        if normalized:
            return normalized
    parent_hex = normalize_hex_id(coin.get("parent_coin_info"))
    puzzle_hex = normalize_hex_id(coin.get("puzzle_hash"))
    amount = safe_int(coin.get("amount"), default=-1)
    if not parent_hex or not puzzle_hex or amount < 0 or amount > 0xFFFFFFFFFFFFFFFF:
        return ""
    try:
        sdk = import_sdk()
        coin_obj = sdk.Coin(hex_to_bytes(parent_hex), hex_to_bytes(puzzle_hex), int(amount))
        return normalize_hex_id(sdk.to_hex(coin_obj.coin_id())) or ""
    except Exception:
        return ""

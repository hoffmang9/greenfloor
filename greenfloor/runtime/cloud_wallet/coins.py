"""Public coin-state helpers shared by Cloud Wallet runtime and CLI."""

from __future__ import annotations


def safe_int(value: object) -> int | None:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return None


def coin_asset_id(coin: dict) -> str:
    asset_raw = coin.get("asset")
    if isinstance(asset_raw, dict):
        return str(asset_raw.get("id", "xch")).strip() or "xch"
    if isinstance(asset_raw, str):
        return asset_raw.strip() or "xch"
    return "xch"


def is_spendable_coin(coin: dict) -> bool:
    if bool(coin.get("isLocked", False)):
        return False
    coin_state = str(coin.get("state", "")).strip().upper()
    if not coin_state:
        return False
    if coin_state in {
        "PENDING",
        "MEMPOOL",
        "SPENT",
        "SPENDING",
        "LOCKED",
        "RESERVED",
        "UNCONFIRMED",
    }:
        return False
    return coin_state in {"CONFIRMED", "UNSPENT", "SPENDABLE", "AVAILABLE", "SETTLED"}

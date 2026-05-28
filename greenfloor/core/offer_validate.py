"""Rust-backed Dexie offer text validation error mapping."""

from __future__ import annotations


def map_validate_offer_error(exc: BaseException) -> str:
    message = str(exc)
    if "offer_duplicate_spent_coin_ids" in message:
        return "wallet_sdk_offer_duplicate_spent_coin_ids"
    if "offer_missing_expiration" in message:
        return "wallet_sdk_offer_missing_expiration"
    return f"wallet_sdk_offer_validate_failed:{exc}"

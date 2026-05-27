"""Public coin-state helpers shared by Cloud Wallet runtime and CLI."""

from __future__ import annotations

from typing import Any


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


def resolve_coin_global_ids(
    wallet_coins: list[dict], raw_coin_ids: list[str]
) -> tuple[list[str], list[str]]:
    """Map operator hex coin names (or Coin_* global IDs) to Cloud Wallet global IDs.

    Returns (resolved_ids, unresolved_ids).  Operators usually copy hex coin names
    from ``coins-list`` output; Cloud Wallet mutations require the ``Coin_*`` GraphQL
    global-ID form.  Direct ``Coin_*`` IDs are passed through unchanged for power users.
    """
    mapping: dict[str, str] = {}
    for coin in wallet_coins:
        global_id = str(coin.get("id", "")).strip()
        name = str(coin.get("name", "")).strip()
        if global_id:
            mapping[global_id] = global_id
        if name and global_id:
            mapping[name] = global_id
    resolved: list[str] = []
    unresolved: list[str] = []
    for raw in raw_coin_ids:
        token = str(raw).strip()
        mapped = mapping.get(token)
        if mapped:
            resolved.append(mapped)
        elif token.startswith("Coin_"):
            resolved.append(token)
        else:
            unresolved.append(token)
    return resolved, unresolved


def coin_id_asset_lookup(wallet_coins: list[dict]) -> dict[str, str]:
    lookup: dict[str, str] = {}
    for coin in wallet_coins:
        coin_id = str(coin.get("id", "")).strip()
        if not coin_id:
            continue
        lookup[coin_id] = coin_asset_id(coin).strip().lower()
    return lookup


def classify_resolved_coin_ids_by_asset(
    *,
    wallet_coins: list[dict],
    resolved_coin_ids: list[str],
    expected_asset_id: str,
) -> tuple[list[str], list[dict[str, str]]]:
    lookup = coin_id_asset_lookup(wallet_coins)
    expected = str(expected_asset_id).strip().lower()
    unknown: list[str] = []
    mismatched: list[dict[str, str]] = []
    for coin_id in resolved_coin_ids:
        normalized_coin_id = str(coin_id).strip()
        actual_asset = lookup.get(normalized_coin_id)
        if actual_asset is None:
            unknown.append(normalized_coin_id)
            continue
        if actual_asset != expected:
            mismatched.append({"coin_id": normalized_coin_id, "coin_asset_id": actual_asset})
    return unknown, mismatched


def coin_matches_direct_spendable_lookup(
    *,
    wallet: Any,
    coin: dict,
    scoped_asset_id: str,
    cache: dict[str, bool] | None = None,
) -> bool:
    get_coin_record = getattr(wallet, "get_coin_record", None)
    if not callable(get_coin_record):
        return True
    coin_id = str(coin.get("id", "")).strip()
    if not coin_id:
        return False
    if cache is not None and coin_id in cache:
        return bool(cache[coin_id])
    try:
        coin_record = get_coin_record(coin_id=coin_id)
    except Exception:
        result = False
    else:
        if not isinstance(coin_record, dict):
            result = False
        else:
            result = (
                is_spendable_coin(coin_record)
                and not bool(coin_record.get("isLinkedToOpenOffer"))
                and coin_asset_id(coin_record).strip().lower()
                == str(scoped_asset_id).strip().lower()
            )
    if cache is not None:
        cache[coin_id] = result
    return result

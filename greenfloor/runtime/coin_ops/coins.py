"""Public coin-state helpers shared by coin-op runtime and CLI."""

from __future__ import annotations

from typing import Any

from greenfloor.core.coin_ops import is_spendable_wallet_coin


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
    return is_spendable_wallet_coin(coin)


def resolve_coin_global_ids(
    wallet_coins: list[dict], raw_coin_ids: list[str]
) -> tuple[list[str], list[str]]:
    """Map operator hex coin names to backend coin global IDs.

    Returns (resolved_ids, unresolved_ids). Operators usually copy hex coin names
    from ``coins-list`` output; some backends require a ``Coin_*`` GraphQL global-ID
    form. Direct ``Coin_*`` IDs are passed through unchanged for power users.
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


def coin_row_is_unlocked_and_unlinked(*, coin: dict) -> bool:
    return not bool(coin.get("isLocked")) and not bool(coin.get("isLinkedToOpenOffer"))


def coin_matches_scoped_asset_id(*, coin: dict, scoped_asset_id: str) -> bool:
    target_asset = str(scoped_asset_id).strip().lower()
    if not target_asset:
        return False
    asset_payload = coin.get("asset")
    if isinstance(asset_payload, dict):
        row_asset_id = str(asset_payload.get("id", "")).strip().lower()
        if row_asset_id:
            return row_asset_id == target_asset
    # Asset-scoped coin queries can omit per-row asset metadata.
    return True


def coin_matches_scoped_spendable_filters(
    *,
    coin: dict,
    scoped_asset_id: str,
    canonical_asset_id: str,
) -> bool:
    from greenfloor.core.coin_ops import coin_meets_coin_op_min_amount

    if not is_spendable_coin(coin):
        return False
    if not coin_row_is_unlocked_and_unlinked(coin=coin):
        return False
    if not coin_matches_scoped_asset_id(coin=coin, scoped_asset_id=scoped_asset_id):
        return False
    return coin_meets_coin_op_min_amount(coin, canonical_asset_id=canonical_asset_id)


def refresh_scoped_spendable_coin_rows(
    *,
    wallet: Any,
    resolved_asset_id: str,
    canonical_asset_id: str,
) -> dict[str, dict] | None:
    scoped_asset_id = str(resolved_asset_id).strip().lower()
    if not scoped_asset_id:
        return None
    try:
        refreshed = wallet.list_coins(asset_id=resolved_asset_id)
    except Exception:
        return None
    spendable_by_id: dict[str, dict] = {}
    for coin in refreshed:
        if not isinstance(coin, dict):
            continue
        coin_id = str(coin.get("id", "")).strip()
        if not coin_id:
            continue
        if not coin_matches_scoped_spendable_filters(
            coin=coin,
            scoped_asset_id=scoped_asset_id,
            canonical_asset_id=canonical_asset_id,
        ):
            continue
        spendable_by_id[coin_id] = coin
    return spendable_by_id


def filter_spendable_scoped_coins(
    *,
    coins: list[dict],
    wallet: Any,
    resolved_asset_id: str,
    canonical_asset_id: str,
    refresh_rows: bool = True,
) -> list[dict]:
    """Return spendable asset-scoped coins (CLI + daemon coin-op selection)."""
    target_asset = str(resolved_asset_id).strip().lower()
    refreshed_rows = (
        refresh_scoped_spendable_coin_rows(
            wallet=wallet,
            resolved_asset_id=resolved_asset_id,
            canonical_asset_id=canonical_asset_id,
        )
        if refresh_rows
        else None
    )
    scoped: list[dict] = []
    for coin in coins:
        if not isinstance(coin, dict):
            continue
        coin_id = str(coin.get("id", "")).strip()
        if not coin_id:
            continue
        candidate_coin = coin
        if refreshed_rows is not None:
            refreshed_coin = refreshed_rows.get(coin_id)
            if refreshed_coin is None:
                continue
            candidate_coin = refreshed_coin
        if not coin_matches_scoped_spendable_filters(
            coin=candidate_coin,
            scoped_asset_id=target_asset,
            canonical_asset_id=canonical_asset_id,
        ):
            continue
        scoped.append(candidate_coin)
    return scoped


def coin_matches_direct_spendable_lookup(
    *,
    wallet: Any,
    coin: dict,
    scoped_asset_id: str,
    cache: dict[str, bool] | None = None,
    fail_open_on_lookup_error: bool = False,
) -> bool:
    get_coin_record = getattr(wallet, "get_coin_record", None)
    if not callable(get_coin_record):
        return True
    coin_id = str(coin.get("id", "")).strip()
    if not coin_id:
        return False
    if cache is not None and coin_id in cache:
        return bool(cache[coin_id])
    fallback_result = bool(
        is_spendable_coin(coin)
        and coin_row_is_unlocked_and_unlinked(coin=coin)
        and coin_matches_scoped_asset_id(coin=coin, scoped_asset_id=scoped_asset_id)
    )
    if not fallback_result and not fail_open_on_lookup_error:
        if cache is not None:
            cache[coin_id] = False
        return False
    try:
        coin_record = get_coin_record(coin_id=coin_id)
    except Exception:
        result = fallback_result if fail_open_on_lookup_error else False
    else:
        if not isinstance(coin_record, dict):
            result = fallback_result if fail_open_on_lookup_error else False
        else:
            asset_payload = coin_record.get("asset")
            row_asset_id = (
                str(asset_payload.get("id", "")).strip().lower()
                if isinstance(asset_payload, dict)
                else ""
            )
            base_match = bool(
                is_spendable_coin(coin_record)
                and coin_row_is_unlocked_and_unlinked(coin=coin_record)
            )
            scoped = str(scoped_asset_id).strip().lower()
            result = base_match and (row_asset_id == scoped if row_asset_id else True)
    if cache is not None:
        cache[coin_id] = result
    return result

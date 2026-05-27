"""Low-level coin selection helpers for coin-operation planning."""

from __future__ import annotations

from greenfloor.core.coin_ops_policy import coin_op_min_amount_mojos


def select_largest_spendable_coin(
    coins: list[dict],
    *,
    min_amount_mojos: int = 0,
    exclude_coin_ids: set[str] | None = None,
) -> dict | None:
    excluded = exclude_coin_ids or set()
    eligible = [
        coin
        for coin in coins
        if isinstance(coin, dict)
        and str(coin.get("id", "")).strip()
        and str(coin.get("id", "")).strip() not in excluded
        and int(coin.get("amount", 0)) >= int(min_amount_mojos)
    ]
    if not eligible:
        return None
    return max(eligible, key=lambda coin: int(coin.get("amount", 0)))


def select_exact_amount_coin_ids(
    coins: list[dict],
    *,
    amount_mojos: int,
    exclude_coin_ids: set[str] | None = None,
    max_count: int | None = None,
) -> list[str]:
    excluded = {value.lower() for value in (exclude_coin_ids or set())}
    selected: list[str] = []
    for coin in coins:
        if not isinstance(coin, dict):
            continue
        coin_id = str(coin.get("id", "")).strip()
        if not coin_id or coin_id.lower() in excluded:
            continue
        try:
            amount = int(coin.get("amount", 0))
        except (TypeError, ValueError):
            continue
        if amount != int(amount_mojos):
            continue
        selected.append(coin_id)
        if max_count is not None and len(selected) >= int(max_count):
            break
    return selected


def split_would_create_sub_cat_change(
    *,
    selected_amount_mojos: int,
    required_amount_mojos: int,
    canonical_asset_id: str,
) -> tuple[bool, int]:
    remainder = int(selected_amount_mojos) - int(required_amount_mojos)
    min_cat_mojos = coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)
    if min_cat_mojos > 0 and remainder > 0 and remainder < int(min_cat_mojos):
        return True, remainder
    return False, remainder

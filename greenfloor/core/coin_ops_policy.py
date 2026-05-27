"""Deterministic coin-operation policy shared by CLI and daemon."""

from __future__ import annotations

from greenfloor.hex_utils import canonical_is_xch


def coin_op_min_amount_mojos(*, canonical_asset_id: str) -> int:
    # Temporary workaround for the upstream Cloud Wallet / ent-wallet asset-scope
    # bug documented in docs/ent-wallet-upstream-byc-coin-query-issue.md.
    # Ignore sub-1-CAT dust during local split/combine candidate selection so
    # tiny stray rows do not get pulled into operational coin management.
    if canonical_is_xch(canonical_asset_id):
        return 0
    return 1000


def coin_meets_coin_op_min_amount(coin: dict, *, canonical_asset_id: str) -> bool:
    try:
        amount = int(coin.get("amount", 0))
    except (TypeError, ValueError):
        return False
    return amount >= coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)


def coin_op_target_amount_allowed(*, amount_mojos: int, canonical_asset_id: str) -> bool:
    return int(amount_mojos) >= coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)

from __future__ import annotations

from greenfloor.core.kernel_bridge import import_kernel


def coin_op_min_amount_mojos(*, canonical_asset_id: str) -> int:
    # Temporary workaround for the upstream Cloud Wallet / ent-wallet asset-scope
    # bug documented in docs/ent-wallet-upstream-byc-coin-query-issue.md.
    # Ignore sub-1-CAT dust during local split/combine candidate selection so
    # tiny stray rows do not get pulled into operational coin management.
    return int(import_kernel().coin_op_min_amount_mojos(str(canonical_asset_id)))


def coin_meets_coin_op_min_amount(coin: dict, *, canonical_asset_id: str) -> bool:
    return bool(import_kernel().coin_meets_coin_op_min_amount(coin, str(canonical_asset_id)))


def coin_op_target_amount_allowed(*, amount_mojos: int, canonical_asset_id: str) -> bool:
    return bool(
        import_kernel().coin_op_target_amount_allowed(
            int(amount_mojos),
            str(canonical_asset_id),
        )
    )

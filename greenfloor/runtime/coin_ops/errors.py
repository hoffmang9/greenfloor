"""Structured coin-operation error payloads (runtime only; CLI prints them)."""

from __future__ import annotations

from typing import Any

from greenfloor.runtime.coin_ops_scope import CoinOpScope, scope_payload


def coin_op_error_payload(
    *,
    scope: CoinOpScope,
    error: str,
    operator_guidance: str,
    **extra: object,
) -> dict[str, object]:
    return {
        **scope_payload(scope),
        "waited": False,
        "success": False,
        "error": error,
        "operator_guidance": operator_guidance,
        **extra,
    }


def coin_op_unresolved_error_payload(
    *,
    scope: CoinOpScope,
    unresolved_coin_ids: list[str],
) -> dict[str, object]:
    guidance = "run greenfloor-manager coins-list and pass coin_id values from output"
    return coin_op_error_payload(
        scope=scope,
        error="coin_id_resolution_failed",
        operator_guidance=guidance,
        unknown_coin_ids=unresolved_coin_ids,
    )


def coin_split_lockup_guardrail_error_payload(
    *,
    scope: CoinOpScope,
    resolved_asset_id: str,
    spendable_asset_coin_ids: set[str],
    selected_coin_ids: list[str],
) -> dict[str, object]:
    selected_spendable_ids = sorted(set(selected_coin_ids) & spendable_asset_coin_ids)
    return coin_op_error_payload(
        scope=scope,
        error="coin_split_guardrail_would_lock_all_spendable_coins",
        operator_guidance=(
            "coin-split would consume all currently spendable coins for this asset; "
            "leave at least one spendable coin free or pass --allow-lock-all-spendable "
            "to override intentionally"
        ),
        resolved_asset_id=resolved_asset_id,
        spendable_asset_coin_count=len(spendable_asset_coin_ids),
        selected_spendable_coin_count=len(selected_spendable_ids),
        selected_spendable_coin_ids=selected_spendable_ids,
    )


def coin_split_no_spendable_error_payload(
    *,
    scope: CoinOpScope,
    canonical_asset_id: str,
    resolved_asset_id: str,
    min_coin_amount_mojos: int,
) -> dict[str, object]:
    return coin_op_error_payload(
        scope=scope,
        error="no_spendable_split_coin_available",
        operator_guidance=(
            "no spendable coins are currently available for this asset; "
            "wait for pending/signature requests to settle or free locked offers, "
            "then retry coin-split. Temporary workaround: CAT split selection "
            "ignores coins smaller than 1 CAT unit (1000 mojos)."
        ),
        asset_id=canonical_asset_id,
        resolved_asset_id=resolved_asset_id,
        temporary_min_coin_amount_mojos=int(min_coin_amount_mojos),
    )


def coin_combine_asset_mismatch_error_payload(
    *,
    scope: CoinOpScope,
    resolved_asset_id: str,
    mismatched_coin_ids: list[dict[str, Any]],
) -> dict[str, object]:
    return coin_op_error_payload(
        scope=scope,
        error="coin_id_asset_mismatch",
        operator_guidance=(
            "all explicit --coin-id values must resolve to the same asset "
            "as --asset-id; re-run coins-list scoped to the target asset "
            "and retry with only those coin ids"
        ),
        resolved_asset_id=resolved_asset_id,
        mismatched_coin_ids=[
            str(entry.get("coin_id", "")).strip()
            for entry in mismatched_coin_ids
            if str(entry.get("coin_id", "")).strip()
        ],
        mismatched_coin_assets=mismatched_coin_ids,
    )


def coin_combine_insufficient_coins_error_payload(
    *,
    scope: CoinOpScope,
    combine_canonical_asset_id: str,
    resolved_asset_id: str,
    required_coin_count: int,
    eligible_coin_count: int,
    min_coin_amount_mojos: int,
) -> dict[str, object]:
    return coin_op_error_payload(
        scope=scope,
        error="insufficient_combine_coins_after_temp_cat_floor",
        operator_guidance=(
            "not enough spendable coins remain after ignoring CAT coins "
            "smaller than 1 CAT unit (1000 mojos). Wait for larger coins, "
            "re-split inventory, or pass explicit --coin-id values if you "
            "intend to override the temporary workaround."
        ),
        asset_id=combine_canonical_asset_id,
        resolved_asset_id=resolved_asset_id,
        required_coin_count=int(required_coin_count),
        eligible_coin_count=int(eligible_coin_count),
        temporary_min_coin_amount_mojos=int(min_coin_amount_mojos),
    )

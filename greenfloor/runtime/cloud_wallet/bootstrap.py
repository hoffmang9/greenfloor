from __future__ import annotations

import collections.abc
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.hex_utils import default_mojo_multiplier_for_asset
from greenfloor.offer_bootstrap import BootstrapLadderEntry, plan_bootstrap_mixed_outputs
from greenfloor.runtime.cloud_wallet.coins import is_spendable_coin
from greenfloor.runtime.cloud_wallet.polling import (
    poll_signature_request_until_not_unsigned,
    wait_for_mempool_then_confirmation,
)
from greenfloor.runtime.coinset_runtime import _resolve_taker_or_coin_operation_fee
from greenfloor.runtime.offer_publish import normalize_offer_side

# Backward-compatible alias for tests importing the private name.
_BootstrapLadderEntry = BootstrapLadderEntry


def bootstrap_fee_cost_for_output_count(output_count: int) -> int:
    count = max(1, int(output_count))
    # Heuristic cost model for Coinset fee advice:
    # - 1_000_000 baseline for a simple bootstrap spend
    # - +250_000 per extra output to bias fee advice upward as fanout grows
    # This is intentionally conservative (not a CLVM consensus constant) and
    # should be tuned empirically from observed mempool/confirmation behavior.
    return 1_000_000 + max(0, count - 1) * 250_000


def resolve_bootstrap_split_fee(
    *,
    network: str,
    minimum_fee_mojos: int,
    output_count: int,
) -> tuple[int, str, str | None]:
    fee_cost = bootstrap_fee_cost_for_output_count(output_count)
    spend_count = max(1, int(output_count))
    try:
        fee_mojos, fee_source = _resolve_taker_or_coin_operation_fee(
            network=network,
            minimum_fee_mojos=minimum_fee_mojos,
            fee_cost=fee_cost,
            spend_count=spend_count,
        )
        return int(fee_mojos), fee_source, None
    except Exception as exc:
        # Preserve the existing fee policy contract: fallback honors
        # `coin_ops.minimum_fee_mojos` exactly, and an explicit zero config
        # value means "allow zero-fee fallback when fee advice is unavailable".
        fallback_fee = max(0, int(minimum_fee_mojos))
        return fallback_fee, "config_minimum_fee_fallback", str(exc)


def ensure_offer_bootstrap_denominations(
    *,
    program: Any,
    market: Any,
    wallet: CloudWalletAdapter,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    quote_price: float,
    action_side: str = "sell",
    bootstrap_signature_wait_timeout_seconds: int = 45,
    bootstrap_signature_warning_interval_seconds: int = 30,
    bootstrap_wait_timeout_seconds: int = 120,
    bootstrap_wait_mempool_warning_seconds: int = 30,
    bootstrap_wait_confirmation_warning_seconds: int = 60,
    plan_bootstrap_mixed_outputs_fn: collections.abc.Callable[..., Any] | None = None,
    resolve_bootstrap_split_fee_fn: collections.abc.Callable[..., tuple[int, str, str | None]]
    | None = None,
    split_coins_fn: collections.abc.Callable[..., dict[str, Any]] | None = None,
    poll_signature_request_until_not_unsigned_fn: collections.abc.Callable[
        ..., tuple[str, list[dict[str, str]]]
    ]
    | None = None,
    wait_for_mempool_then_confirmation_fn: collections.abc.Callable[..., list[dict[str, str]]]
    | None = None,
    is_spendable_coin_fn: collections.abc.Callable[[dict], bool] | None = None,
) -> dict[str, Any]:
    if plan_bootstrap_mixed_outputs_fn is None:
        plan_bootstrap_mixed_outputs_fn = plan_bootstrap_mixed_outputs
    if resolve_bootstrap_split_fee_fn is None:
        resolve_bootstrap_split_fee_fn = resolve_bootstrap_split_fee
    if split_coins_fn is None:
        split_coins_fn = getattr(wallet, "split_coins", None)
    if poll_signature_request_until_not_unsigned_fn is None:
        poll_signature_request_until_not_unsigned_fn = poll_signature_request_until_not_unsigned
    if wait_for_mempool_then_confirmation_fn is None:
        wait_for_mempool_then_confirmation_fn = wait_for_mempool_then_confirmation
    if is_spendable_coin_fn is None:
        is_spendable_coin_fn = is_spendable_coin

    side = normalize_offer_side(action_side)
    ladders = getattr(market, "ladders", {}) or {}
    side_ladder = list(ladders.get(side, []) or []) if isinstance(ladders, dict) else []
    if not side_ladder:
        return {"status": "skipped", "reason": f"missing_{side}_ladder"}

    pricing = dict(getattr(market, "pricing", {}) or {})
    quote_unit_multiplier = int(
        pricing.get(
            "quote_unit_mojo_multiplier",
            default_mojo_multiplier_for_asset(str(resolved_quote_asset_id)),
        )
    )
    if side == "buy":
        ladder_for_split = []
        for entry in side_ladder:
            quote_amount = int(
                round(float(entry.size_base_units) * float(quote_price) * quote_unit_multiplier)
            )
            if quote_amount <= 0:
                continue
            ladder_for_split.append(
                BootstrapLadderEntry(
                    size_base_units=quote_amount,
                    target_count=int(entry.target_count),
                    split_buffer_count=int(entry.split_buffer_count),
                )
            )
        split_asset_id = str(resolved_quote_asset_id).strip()
    else:
        ladder_for_split = side_ladder
        split_asset_id = str(resolved_base_asset_id).strip()

    if not split_asset_id:
        return {"status": "skipped", "reason": f"missing_{side}_asset_for_bootstrap"}

    if not hasattr(wallet, "list_coins"):
        return {
            "status": "skipped",
            "reason": "wallet_list_coins_unavailable_for_bootstrap",
            "fallback_to_cloud_wallet_offer_split": True,
        }

    asset_scoped_coins = wallet.list_coins(asset_id=split_asset_id)
    spendable_asset_coins = [coin for coin in asset_scoped_coins if is_spendable_coin_fn(coin)]
    bootstrap_plan = plan_bootstrap_mixed_outputs_fn(
        sell_ladder=ladder_for_split,
        spendable_coins=spendable_asset_coins,
    )
    if bootstrap_plan is None:
        return {"status": "skipped", "reason": "already_ready"}

    fee_mojos, fee_source, fee_lookup_error = resolve_bootstrap_split_fee_fn(
        network=str(program.app_network),
        minimum_fee_mojos=int(program.coin_ops_minimum_fee_mojos),
        output_count=len(bootstrap_plan.output_amounts_base_units),
    )
    existing_coin_ids = {
        str(c.get("id", "")).strip() for c in asset_scoped_coins if str(c.get("id", "")).strip()
    }
    selected_deficit = max(
        bootstrap_plan.deficits,
        key=lambda row: (int(row.size_base_units), int(row.deficit_count)),
    )
    amount_per_coin = int(selected_deficit.size_base_units)
    desired_coin_count = max(2, int(selected_deficit.deficit_count))
    max_coin_count = int(bootstrap_plan.source_amount) // max(1, amount_per_coin)
    number_of_coins = min(desired_coin_count, max_coin_count)
    if number_of_coins < 2:
        return {
            "status": "failed",
            "reason": "bootstrap_failed:insufficient_source_coin_for_cloud_wallet_split",
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
            "plan": {
                "source_coin_id": bootstrap_plan.source_coin_id,
                "source_amount": bootstrap_plan.source_amount,
                "target_size_base_units": amount_per_coin,
                "requested_coin_count": desired_coin_count,
                "max_coin_count_from_source": max_coin_count,
            },
        }
    if split_coins_fn is None:
        return {"status": "failed", "reason": "split_coins_not_available"}

    try:
        split_result = split_coins_fn(
            coin_ids=[bootstrap_plan.source_coin_id],
            amount_per_coin=amount_per_coin,
            number_of_coins=number_of_coins,
            fee=int(fee_mojos),
        )
    except Exception as exc:
        return {
            "status": "failed",
            "reason": f"bootstrap_failed:cloud_wallet_split_error:{exc}",
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
            "plan": {
                "source_coin_id": bootstrap_plan.source_coin_id,
                "source_amount": bootstrap_plan.source_amount,
                "target_size_base_units": amount_per_coin,
                "coin_count": number_of_coins,
            },
        }

    signature_request_id = str(split_result.get("signature_request_id", "")).strip()
    if not signature_request_id:
        return {
            "status": "failed",
            "reason": "bootstrap_failed:missing_signature_request_id",
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
        }

    signature_events: list[dict[str, str]] = []
    try:
        signature_state, signature_events = poll_signature_request_until_not_unsigned_fn(
            wallet=wallet,
            signature_request_id=signature_request_id,
            timeout_seconds=max(5, int(bootstrap_signature_wait_timeout_seconds)),
            warning_interval_seconds=max(5, int(bootstrap_signature_warning_interval_seconds)),
        )
    except Exception as exc:
        return {
            "status": "failed",
            "reason": "bootstrap_signature_wait_failed",
            "signature_request_id": signature_request_id,
            "signature_wait_error": str(exc),
            "signature_wait_events": signature_events,
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
        }

    wait_events: list[dict[str, str]] = []
    wait_error: str | None = None
    try:
        wait_events = wait_for_mempool_then_confirmation_fn(
            wallet=wallet,
            network=str(program.app_network),
            initial_coin_ids=existing_coin_ids,
            asset_id=split_asset_id,
            mempool_warning_seconds=max(10, int(bootstrap_wait_mempool_warning_seconds)),
            confirmation_warning_seconds=max(10, int(bootstrap_wait_confirmation_warning_seconds)),
            timeout_seconds=max(10, int(bootstrap_wait_timeout_seconds)),
        )
    except Exception as exc:
        wait_error = str(exc)
        return {
            "status": "failed",
            "reason": "bootstrap_wait_failed",
            "wait_error": wait_error,
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
            "plan": {
                "source_coin_id": bootstrap_plan.source_coin_id,
                "source_amount": bootstrap_plan.source_amount,
                "output_count": len(bootstrap_plan.output_amounts_base_units),
                "total_output_amount": bootstrap_plan.total_output_amount,
                "change_amount": bootstrap_plan.change_amount,
            },
            "signature_request_id": signature_request_id,
            "signature_state": signature_state,
            "signature_wait_events": signature_events,
            "wait_events": wait_events,
        }

    refreshed_asset_coins = wallet.list_coins(asset_id=split_asset_id)
    refreshed_spendable = [coin for coin in refreshed_asset_coins if is_spendable_coin_fn(coin)]
    remaining_plan = plan_bootstrap_mixed_outputs_fn(
        sell_ladder=ladder_for_split,
        spendable_coins=refreshed_spendable,
    )
    return {
        "status": "executed",
        "reason": "bootstrap_submitted",
        "ready": remaining_plan is None,
        "fee_mojos": int(fee_mojos),
        "fee_source": fee_source,
        "fee_lookup_error": fee_lookup_error,
        "wait_error": wait_error,
        "plan": {
            "source_coin_id": bootstrap_plan.source_coin_id,
            "source_amount": bootstrap_plan.source_amount,
            "output_count": len(bootstrap_plan.output_amounts_base_units),
            "total_output_amount": bootstrap_plan.total_output_amount,
            "change_amount": bootstrap_plan.change_amount,
            "deficits": [
                {
                    "size_base_units": d.size_base_units,
                    "required_count": d.required_count,
                    "current_count": d.current_count,
                    "deficit_count": d.deficit_count,
                }
                for d in bootstrap_plan.deficits
            ],
        },
        "signature_request_id": signature_request_id,
        "signature_state": signature_state,
        "signature_wait_events": signature_events,
        "wait_events": wait_events,
    }

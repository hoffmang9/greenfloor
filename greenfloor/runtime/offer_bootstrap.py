"""Bootstrap ladder shaping and mixed-split execution for signer offer runtime."""

from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Any

from greenfloor.adapters import rust_signer
from greenfloor.config.models import MarketLadderEntry, ProgramConfig
from greenfloor.core.offer_bootstrap_policy import BootstrapLadderEntry, BootstrapPlan
from greenfloor.core.signer_offer_request import (
    quote_mojos_for_base_size,
    resolve_quote_unit_multiplier,
)


@dataclass(frozen=True)
class BootstrapSplitExecution:
    """Inputs for one vault mixed-split bootstrap submission."""

    program: ProgramConfig
    config_path: str
    bootstrap_plan: BootstrapPlan
    split_asset_id: str
    receive_address: str
    fee_mojos: int
    fee_source: str
    fee_lookup_error: str | None
    existing_coin_ids: set[str]
    bootstrap_wait_timeout_seconds: int
    ladder_for_split: list[BootstrapLadderEntry]
    list_bootstrap_coins_fn: Callable[..., list[dict[str, Any]]]
    wait_for_confirmation_fn: Callable[..., list[dict[str, str]]]
    is_spendable_coin_fn: Callable[[dict], bool]
    plan_bootstrap_mixed_outputs_fn: Callable[..., BootstrapPlan | None]


def bootstrap_ladder_entries_for_side(
    *,
    side: str,
    side_ladder: Sequence[MarketLadderEntry],
    pricing: dict[str, Any],
    quote_price: float,
    resolved_quote_asset_id: str,
) -> list[BootstrapLadderEntry]:
    """Normalize market ladder rows into planner inputs for sell or buy bootstrap."""
    quote_unit_multiplier: int | None = None
    if side == "buy":
        quote_unit_multiplier = resolve_quote_unit_multiplier(
            pricing=pricing,
            resolved_quote_asset_id=str(resolved_quote_asset_id),
        )

    entries: list[BootstrapLadderEntry] = []
    for entry in side_ladder:
        size_base_units = int(entry.size_base_units)
        if quote_unit_multiplier is not None:
            size_base_units = quote_mojos_for_base_size(
                size_base_units=size_base_units,
                quote_price=float(quote_price),
                quote_unit_multiplier=quote_unit_multiplier,
            )
            if size_base_units <= 0:
                continue
        entries.append(
            BootstrapLadderEntry(
                size_base_units=size_base_units,
                target_count=int(entry.target_count),
                split_buffer_count=int(entry.split_buffer_count),
            )
        )
    return entries


def execute_bootstrap_mixed_split(execution: BootstrapSplitExecution) -> dict[str, Any]:
    """Submit vault mixed-split from a planner result, wait, and report readiness."""
    bootstrap_plan = execution.bootstrap_plan
    output_amounts = [int(amount) for amount in bootstrap_plan.output_amounts_base_units]
    split_request = {
        "receive_address": execution.receive_address,
        "asset_id": execution.split_asset_id.removeprefix("0x"),
        "output_amounts": output_amounts,
        "coin_ids": [bootstrap_plan.source_coin_id.removeprefix("0x")],
        "allow_sub_cat_output": False,
        "fee_mojos": 0,
        "broadcast": True,
    }
    try:
        split_result = rust_signer.build_mixed_split(execution.config_path, split_request)
    except Exception as exc:
        return {
            "status": "failed",
            "reason": f"bootstrap_failed:signer_mixed_split_error:{exc}",
            "fee_mojos": int(execution.fee_mojos),
            "fee_source": execution.fee_source,
            "fee_lookup_error": execution.fee_lookup_error,
        }

    wait_events: list[dict[str, str]] = []
    wait_error: str | None = None
    try:
        wait_events = execution.wait_for_confirmation_fn(
            network=str(execution.program.app_network),
            receive_address=execution.receive_address,
            asset_id=execution.split_asset_id,
            initial_coin_ids=execution.existing_coin_ids,
            timeout_seconds=max(10, int(execution.bootstrap_wait_timeout_seconds)),
        )
    except Exception as exc:
        wait_error = str(exc)
        return {
            "status": "failed",
            "reason": "bootstrap_wait_failed",
            "wait_error": wait_error,
            "fee_mojos": int(execution.fee_mojos),
            "fee_source": execution.fee_source,
            "fee_lookup_error": execution.fee_lookup_error,
            "split_result": dict(split_result) if isinstance(split_result, dict) else {},
            "wait_events": wait_events,
        }

    refreshed_asset_coins = execution.list_bootstrap_coins_fn(
        network=str(execution.program.app_network),
        receive_address=execution.receive_address,
        asset_id=execution.split_asset_id,
    )
    refreshed_spendable = [
        coin for coin in refreshed_asset_coins if execution.is_spendable_coin_fn(coin)
    ]
    remaining_plan = execution.plan_bootstrap_mixed_outputs_fn(
        sell_ladder=execution.ladder_for_split,
        spendable_coins=refreshed_spendable,
    )
    return {
        "status": "executed",
        "reason": "bootstrap_submitted",
        "ready": remaining_plan is None,
        "fee_mojos": int(execution.fee_mojos),
        "fee_source": execution.fee_source,
        "fee_lookup_error": execution.fee_lookup_error,
        "wait_error": wait_error,
        "split_result": dict(split_result) if isinstance(split_result, dict) else {},
        "wait_events": wait_events,
        "plan": {
            "source_coin_id": bootstrap_plan.source_coin_id,
            "source_amount": bootstrap_plan.source_amount,
            "output_count": len(bootstrap_plan.output_amounts_base_units),
            "total_output_amount": bootstrap_plan.total_output_amount,
            "change_amount": bootstrap_plan.change_amount,
        },
    }

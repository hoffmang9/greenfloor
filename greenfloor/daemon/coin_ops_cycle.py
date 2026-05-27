"""Daemon coin-op planning and execution for a market cycle."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.core.coin_ops import BucketSpec, plan_coin_ops
from greenfloor.core.coin_ops_policy import (
    coin_op_min_amount_mojos as _coin_op_min_amount_mojos,
)
from greenfloor.core.coin_ops_policy import (
    coin_op_target_amount_allowed as _coin_op_target_amount_allowed,
)
from greenfloor.core.fee_budget import partition_plans_by_budget, projected_coin_ops_fee_mojos
from greenfloor.daemon.cooldowns import _combine_input_coin_cap
from greenfloor.daemon.market_logging import _daemon_logger, _log_market_decision
from greenfloor.daemon.offer_execution import _normalize_offer_side, _watched_coin_ids_for_market
from greenfloor.hex_utils import default_mojo_multiplier_for_asset
from greenfloor.runtime.coin_ops.daemon_execution import execute_managed_coin_op_plans
from greenfloor.storage.sqlite import SqliteStore


def _effective_sell_bucket_counts_for_coin_ops(
    *,
    sell_ladder: list[Any],
    wallet_bucket_counts: dict[int, int],
    active_sell_offer_counts_by_size: dict[int, int] | None,
    newly_executed_sell_offer_counts_by_size: dict[int, int] | None = None,
) -> dict[int, int]:
    effective_counts = dict(wallet_bucket_counts)
    active_sell_counts = active_sell_offer_counts_by_size or {}
    newly_executed_sell_counts = newly_executed_sell_offer_counts_by_size or {}
    for entry in sell_ladder:
        size_base_units = int(getattr(entry, "size_base_units", 0))
        if size_base_units <= 0:
            continue
        target_count = max(0, int(getattr(entry, "target_count", 0)))
        newly_executed_sell_count = max(0, int(newly_executed_sell_counts.get(size_base_units, 0)))
        wallet_count = max(
            0,
            int(wallet_bucket_counts.get(size_base_units, 0)) - newly_executed_sell_count,
        )
        active_sell_count = max(0, int(active_sell_counts.get(size_base_units, 0)))
        effective_active_sell_count = active_sell_count + newly_executed_sell_count
        # Count live sell offers toward the market target, but not toward the
        # split buffer. That preserves at most one extra ready coin above the
        # active sell ladder coverage.
        effective_counts[size_base_units] = wallet_count + min(
            effective_active_sell_count,
            target_count,
        )
    return effective_counts


def _executed_sell_offer_counts_by_size(offer_execution: dict[str, Any]) -> dict[int, int]:
    counts: dict[int, int] = {}
    items = offer_execution.get("items", [])
    if not isinstance(items, list):
        return counts
    for item in items:
        if not isinstance(item, dict):
            continue
        if str(item.get("status", "")).strip().lower() != "executed":
            continue
        if _normalize_offer_side(item.get("side", "sell")) != "sell":
            continue
        try:
            size = int(item.get("size", 0))
        except (TypeError, ValueError):
            continue
        if size <= 0:
            continue
        counts[size] = counts.get(size, 0) + 1
    return counts


def _base_unit_mojo_multiplier_for_market(*, market: Any) -> int:
    pricing = getattr(market, "pricing", {}) or {}
    default_multiplier = default_mojo_multiplier_for_asset(str(getattr(market, "base_asset", "")))
    try:
        multiplier = int(pricing.get("base_unit_mojo_multiplier", default_multiplier))
    except (TypeError, ValueError):
        multiplier = default_multiplier
    return max(1, multiplier)


def _plan_and_execute_coin_ops(
    *,
    market: Any,
    program: Any,
    wallet: WalletAdapter,
    store: SqliteStore,
    sell_ladder: list[Any],
    wallet_bucket_counts: dict[int, int],
    active_sell_offer_counts_by_size: dict[int, int] | None,
    newly_executed_sell_offer_counts_by_size: dict[int, int] | None,
    signer_selection: Any,
    state_dir: Path,
) -> None:
    """Plan and execute coin split/combine operations for a market."""
    bucket_counts = _effective_sell_bucket_counts_for_coin_ops(
        sell_ladder=sell_ladder,
        wallet_bucket_counts=wallet_bucket_counts,
        active_sell_offer_counts_by_size=active_sell_offer_counts_by_size,
        newly_executed_sell_offer_counts_by_size=newly_executed_sell_offer_counts_by_size,
    )
    base_unit_mojo_multiplier = _base_unit_mojo_multiplier_for_market(market=market)
    canonical_base_asset_id = str(getattr(market, "base_asset", "")).strip()
    invalid_buckets: list[dict[str, int]] = []
    valid_sell_ladder: list[Any] = []
    for entry in sell_ladder:
        size_base_units = int(getattr(entry, "size_base_units", 0))
        if size_base_units <= 0:
            continue
        target_amount_mojos = size_base_units * int(base_unit_mojo_multiplier)
        if _coin_op_target_amount_allowed(
            amount_mojos=target_amount_mojos,
            canonical_asset_id=canonical_base_asset_id,
        ):
            valid_sell_ladder.append(entry)
            continue
        invalid_buckets.append(
            {
                "size_base_units": size_base_units,
                "target_amount_mojos": int(target_amount_mojos),
                "minimum_allowed_mojos": int(
                    _coin_op_min_amount_mojos(canonical_asset_id=canonical_base_asset_id)
                ),
            }
        )
    if invalid_buckets:
        _log_market_decision(
            market.market_id,
            "coin_ops_skip_sub_minimum_target_amount",
            invalid_bucket_count=len(invalid_buckets),
            invalid_buckets=invalid_buckets,
        )
    if not valid_sell_ladder:
        return
    buckets = [
        BucketSpec(
            size_base_units=e.size_base_units,
            target_count=e.target_count,
            split_buffer_count=e.split_buffer_count,
            combine_when_excess_factor=e.combine_when_excess_factor,
            current_count=int(bucket_counts.get(e.size_base_units, 0)),
        )
        for e in valid_sell_ladder
    ]
    plans = plan_coin_ops(
        buckets=buckets,
        max_operations_per_run=program.coin_ops_max_operations_per_run,
        max_fee_budget_mojos=program.coin_ops_max_daily_fee_budget_mojos,
        split_fee_mojos=program.coin_ops_split_fee_mojos,
        combine_fee_mojos=program.coin_ops_combine_fee_mojos,
    )
    if plans:
        _log_market_decision(
            market.market_id,
            "coin_ops_planned",
            plan_count=len(plans),
            split_plan_count=sum(1 for p in plans if str(p.op_type) == "split"),
            combine_plan_count=sum(1 for p in plans if str(p.op_type) == "combine"),
            split_op_count=sum(int(p.op_count) for p in plans if str(p.op_type) == "split"),
            combine_op_count=sum(int(p.op_count) for p in plans if str(p.op_type) == "combine"),
        )
        projected_fee = projected_coin_ops_fee_mojos(
            plans=plans,
            split_fee_mojos=program.coin_ops_split_fee_mojos,
            combine_fee_mojos=program.coin_ops_combine_fee_mojos,
        )
        spent_today = store.get_daily_fee_spent_mojos_utc()
        executable_plans, overflow_plans = partition_plans_by_budget(
            plans=plans,
            split_fee_mojos=program.coin_ops_split_fee_mojos,
            combine_fee_mojos=program.coin_ops_combine_fee_mojos,
            spent_today_mojos=spent_today,
            max_daily_fee_budget_mojos=program.coin_ops_max_daily_fee_budget_mojos,
        )
        if executable_plans:
            execution = execute_managed_coin_op_plans(
                market=market,
                program=program,
                plans=executable_plans,
                signer_selection=signer_selection,
                base_unit_mojo_multiplier=_base_unit_mojo_multiplier_for_market(market=market),
                combine_input_cap=_combine_input_coin_cap(),
                watched_coin_ids=_watched_coin_ids_for_market(
                    market_id=str(getattr(market, "market_id", "")).strip()
                ),
                logger=_daemon_logger,
            )
            _log_market_decision(
                market.market_id,
                "coin_ops_executed",
                plan_count=len(plans),
                executable_count=len(executable_plans),
                overflow_count=len(overflow_plans),
            )
        else:
            execution = {
                "dry_run": program.runtime_dry_run,
                "planned_count": 0,
                "executed_count": 0,
                "status": "skipped_fee_budget",
                "items": [],
            }
            _log_market_decision(
                market.market_id,
                "coin_ops_skipped_fee_budget",
                plan_count=len(plans),
                overflow_count=len(overflow_plans),
            )
        if overflow_plans:
            store.add_audit_event(
                "coin_ops_partial_or_skipped_fee_budget",
                {
                    "market_id": market.market_id,
                    "spent_today_mojos": spent_today,
                    "projected_mojos": projected_fee,
                    "max_daily_fee_budget_mojos": program.coin_ops_max_daily_fee_budget_mojos,
                    "overflow_plans": [
                        {
                            "op_type": p.op_type,
                            "size_base_units": p.size_base_units,
                            "op_count": p.op_count,
                            "reason": p.reason,
                        }
                        for p in overflow_plans
                    ],
                },
                market_id=market.market_id,
            )
            execution_items = execution.get("items", [])
            execution_items.extend(
                [
                    {
                        "op_type": p.op_type,
                        "size_base_units": p.size_base_units,
                        "op_count": p.op_count,
                        "status": "skipped",
                        "reason": "fee_budget_guard",
                        "operation_id": None,
                    }
                    for p in overflow_plans
                ]
            )
            execution["items"] = execution_items
        execution["planned_count"] = len(plans)
        store.add_audit_event(
            "coin_ops_plan",
            {
                "market_id": market.market_id,
                "projected_fee_mojos": projected_fee,
                "spent_today_mojos": spent_today,
                "plans": [
                    {
                        "op_type": p.op_type,
                        "size_base_units": p.size_base_units,
                        "op_count": p.op_count,
                        "reason": p.reason,
                    }
                    for p in plans
                ],
                "execution": execution,
            },
            market_id=market.market_id,
        )
        for item in execution.get("items", []):
            event_type = f"coin_op_{item.get('status', 'unknown')}"
            op_type = str(item.get("op_type"))
            per_op_fee = (
                program.coin_ops_split_fee_mojos
                if op_type == "split"
                else program.coin_ops_combine_fee_mojos
            )
            op_count = int(item.get("op_count", 0))
            fee_mojos = per_op_fee * op_count if item.get("status") == "executed" else 0
            _log_market_decision(
                market.market_id,
                "coin_op_item_result",
                op_type=op_type,
                status=str(item.get("status", "unknown")),
                op_count=op_count,
                size_base_units=item.get("size_base_units"),
                reason=str(item.get("reason", "")),
                operation_id=item.get("operation_id"),
                fee_mojos=fee_mojos,
            )
            store.add_audit_event(
                event_type,
                {
                    "market_id": market.market_id,
                    "op_type": op_type,
                    "size_base_units": item.get("size_base_units"),
                    "op_count": op_count,
                    "reason": item.get("reason"),
                    "operation_id": item.get("operation_id"),
                    "fee_mojos": fee_mojos,
                },
                market_id=market.market_id,
            )
            store.add_coin_op_ledger_entry(
                market_id=market.market_id,
                op_type=op_type,
                op_count=op_count,
                fee_mojos=fee_mojos,
                status=str(item.get("status", "unknown")),
                reason=str(item.get("reason", "")),
                operation_id=(
                    str(item.get("operation_id")) if item.get("operation_id") is not None else None
                ),
            )
    else:
        _log_market_decision(market.market_id, "coin_ops_no_plans")

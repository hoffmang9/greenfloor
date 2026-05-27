"""Daemon Cloud Wallet coin-op plan execution (planning/ledger stay in daemon)."""

from __future__ import annotations

import logging
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.coin_ops import CoinOpPlan
from greenfloor.core.coin_ops_policy import (
    coin_op_min_amount_mojos,
    coin_op_target_amount_allowed,
)
from greenfloor.runtime.cloud_wallet.coin_ops_daemon_ledger import (
    DaemonCoinOpLedgerItem,
    daemon_coin_op_executed,
    daemon_coin_op_skipped,
)
from greenfloor.runtime.cloud_wallet.coin_ops_execution import combine_coins_with_retry
from greenfloor.runtime.cloud_wallet.coin_ops_planning import (
    CombineInputSelectionMode,
    SplitCoinPlan,
    SplitCombinePrereqPlan,
    SplitPlanningProfile,
    SplitSkipPlan,
    plan_auto_combine_inputs,
    plan_auto_split_selection,
)

__all__ = [
    "DaemonCoinOpExecContext",
    "execute_daemon_combine_plan",
    "execute_daemon_split_plan",
]


@dataclass(slots=True)
class DaemonCoinOpExecContext:
    cloud_wallet: CloudWalletAdapter
    market: MarketConfig
    program: ProgramConfig
    resolved_base_asset_id: str
    base_unit_mojo_multiplier: int
    combine_input_cap: int
    spendable_coins: Callable[[list[dict[str, Any]]], list[dict[str, Any]]]
    watched_coin_ids: set[str]
    logger: logging.Logger


def _ledger_rows(items: list[DaemonCoinOpLedgerItem]) -> list[dict[str, Any]]:
    return [item.to_dict() for item in items]


def _submit_combine_prereq_for_split(
    *,
    op_type: str,
    size_base_units: int,
    op_count: int,
    required_amount: int,
    prereq: SplitCombinePrereqPlan,
    ctx: DaemonCoinOpExecContext,
) -> tuple[list[dict[str, Any]], int]:
    if prereq.cap_applied and prereq.selected_total < required_amount:
        ctx.logger.info(
            "coin_ops_combine_cap_progress "
            "market_id=%s required_amount=%s selected_total=%s "
            "selected_before_cap=%s selected_after_cap=%s input_coin_cap=%s "
            "note=%s",
            str(ctx.market.market_id).strip() or "unknown",
            int(required_amount),
            int(prereq.selected_total),
            int(prereq.selected_count_before_cap),
            int(len(prereq.input_coin_ids)),
            int(prereq.combine_input_cap),
            "submitted capped progress combine; next cycle likely needs only 2-coin combine",
        )
    try:
        combine_result = combine_coins_with_retry(
            cloud_wallet=ctx.cloud_wallet,
            combine_kwargs={
                "number_of_coins": len(prereq.input_coin_ids),
                "fee": int(ctx.program.coin_ops_combine_fee_mojos),
                "asset_id": ctx.resolved_base_asset_id,
                "largest_first": True,
                "input_coin_ids": prereq.input_coin_ids,
                "target_amount": prereq.target_amount,
            },
        )
    except Exception as exc:
        return _ledger_rows(
            [
                daemon_coin_op_skipped(
                    op_type=op_type,
                    size_base_units=size_base_units,
                    op_count=op_count,
                    reason=f"cloud_wallet_coin_op_error:{exc}:combine_for_split_prereq",
                )
            ]
        ), 0
    combine_sig_id = str(combine_result.get("signature_request_id", "")).strip()
    if not combine_sig_id:
        return _ledger_rows(
            [
                daemon_coin_op_skipped(
                    op_type=op_type,
                    size_base_units=size_base_units,
                    op_count=op_count,
                    reason="combine_missing_signature_request_id_for_split_prereq",
                )
            ]
        ), 0
    return _ledger_rows(
        [
            daemon_coin_op_executed(
                op_type="combine",
                size_base_units=size_base_units,
                op_count=len(prereq.input_coin_ids),
                reason=(
                    "cloud_wallet_kms_combine_submitted_for_split_prereq_exact"
                    if prereq.exact_match
                    else "cloud_wallet_kms_combine_submitted_for_split_prereq_with_change"
                ),
                operation_id=combine_sig_id,
                data={
                    "target_amount": required_amount,
                    "selected_total": int(prereq.selected_total),
                    "exact_match": bool(prereq.exact_match),
                    "input_coin_cap_applied": bool(prereq.cap_applied),
                    "input_coin_cap": int(prereq.combine_input_cap),
                    "selected_coin_count_before_cap": prereq.selected_count_before_cap,
                    "selected_coin_count_after_cap": len(prereq.input_coin_ids),
                    "next_step_note": (
                        "submitted capped progress combine; next cycle likely needs "
                        "only 2-coin combine"
                        if prereq.cap_applied and prereq.selected_total < required_amount
                        else ""
                    ),
                },
            )
        ]
    ), 1


def execute_daemon_split_plan(
    *,
    plan: CoinOpPlan,
    ctx: DaemonCoinOpExecContext,
) -> tuple[list[dict[str, Any]], int]:
    op_type = str(plan.op_type)
    op_count = int(plan.op_count)
    size_base_units = int(plan.size_base_units)
    items: list[DaemonCoinOpLedgerItem] = []
    executed_count = 0

    if op_count == 1:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="split_single_coin_noop_skipped",
            )
        )
        return _ledger_rows(items), executed_count

    amount_per_coin_mojos = size_base_units * ctx.base_unit_mojo_multiplier
    canonical_asset_id = str(ctx.market.base_asset).strip()
    if not coin_op_target_amount_allowed(
        amount_mojos=amount_per_coin_mojos,
        canonical_asset_id=canonical_asset_id,
    ):
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="split_amount_below_coin_op_minimum",
                data={
                    "amount_per_coin_mojos": int(amount_per_coin_mojos),
                    "minimum_allowed_mojos": int(
                        coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)
                    ),
                },
            )
        )
        return _ledger_rows(items), executed_count

    required_amount = amount_per_coin_mojos * op_count
    coins = ctx.cloud_wallet.list_coins(asset_id=ctx.resolved_base_asset_id)
    if not ctx.spendable_coins(coins):
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="no_spendable_split_coin_available",
            )
        )
        return _ledger_rows(items), executed_count

    attempted_coin_ids: set[str] = set()
    split_submitted = False
    for attempt_index in range(2):
        fresh = ctx.cloud_wallet.list_coins(asset_id=ctx.resolved_base_asset_id)
        candidate_spendable = [
            coin
            for coin in ctx.spendable_coins(fresh)
            if str(coin.get("id", "")).strip() not in attempted_coin_ids
        ]
        selection = plan_auto_split_selection(
            candidate_spendable=candidate_spendable,
            required_amount_mojos=required_amount,
            canonical_asset_id=canonical_asset_id,
            profile=SplitPlanningProfile.DAEMON_AUTO,
            combine_input_cap=ctx.combine_input_cap,
            allow_combine_prereq=(attempt_index == 0),
        )
        if isinstance(selection, SplitCombinePrereqPlan):
            return _submit_combine_prereq_for_split(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                required_amount=required_amount,
                prereq=selection,
                ctx=ctx,
            )
        if isinstance(selection, SplitSkipPlan):
            if selection.reason == "no_spendable_split_coin_meets_required_amount":
                break
            items.append(
                daemon_coin_op_skipped(
                    op_type=op_type,
                    size_base_units=size_base_units,
                    op_count=op_count,
                    reason=selection.reason,
                    data=selection.data,
                )
            )
            return _ledger_rows(items), executed_count
        assert isinstance(selection, SplitCoinPlan)
        selected_coin_id = selection.coin_id
        attempted_coin_ids.add(selected_coin_id)
        try:
            result = ctx.cloud_wallet.split_coins(
                coin_ids=[selected_coin_id],
                amount_per_coin=amount_per_coin_mojos,
                number_of_coins=op_count,
                fee=int(ctx.program.coin_ops_split_fee_mojos),
            )
        except Exception as exc:
            error_text = str(exc)
            if "Some selected coins are not spendable" in error_text and attempt_index == 0:
                continue
            items.append(
                daemon_coin_op_skipped(
                    op_type=op_type,
                    size_base_units=size_base_units,
                    op_count=op_count,
                    reason=(
                        f"cloud_wallet_coin_op_error:{exc}:selected_coin_id={selected_coin_id}"
                    ),
                )
            )
            return _ledger_rows(items), executed_count

        signature_request_id = str(result.get("signature_request_id", "")).strip()
        if not signature_request_id:
            items.append(
                daemon_coin_op_skipped(
                    op_type=op_type,
                    size_base_units=size_base_units,
                    op_count=op_count,
                    reason="split_missing_signature_request_id",
                )
            )
            return _ledger_rows(items), executed_count
        items.append(
            daemon_coin_op_executed(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="cloud_wallet_kms_split_submitted",
                operation_id=signature_request_id,
            )
        )
        split_submitted = True
        executed_count = 1
        break

    if not split_submitted:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="no_spendable_split_coin_meets_required_amount",
            )
        )
    return _ledger_rows(items), executed_count


def execute_daemon_combine_plan(
    *,
    plan: CoinOpPlan,
    ctx: DaemonCoinOpExecContext,
) -> tuple[list[dict[str, Any]], int]:
    op_type = str(plan.op_type)
    op_count = int(plan.op_count)
    size_base_units = int(plan.size_base_units)
    requested_number_of_coins = max(2, op_count)
    capped_number_of_coins = min(requested_number_of_coins, ctx.combine_input_cap)
    target_coin_amount_mojos = size_base_units * ctx.base_unit_mojo_multiplier
    canonical_asset_id = str(ctx.market.base_asset).strip()
    items: list[DaemonCoinOpLedgerItem] = []

    if not coin_op_target_amount_allowed(
        amount_mojos=target_coin_amount_mojos,
        canonical_asset_id=canonical_asset_id,
    ):
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="combine_target_amount_below_coin_op_minimum",
                data={
                    "target_coin_amount_mojos": int(target_coin_amount_mojos),
                    "minimum_allowed_mojos": int(
                        coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)
                    ),
                },
            )
        )
        return _ledger_rows(items), 0

    combine_input_coin_ids = plan_auto_combine_inputs(
        spendable_coins=ctx.spendable_coins(
            ctx.cloud_wallet.list_coins(asset_id=ctx.resolved_base_asset_id)
        ),
        number_of_coins=requested_number_of_coins,
        selection_mode=CombineInputSelectionMode.EXACT_AMOUNT,
        target_amount_mojos=target_coin_amount_mojos,
        exclude_coin_ids=ctx.watched_coin_ids,
        max_count=capped_number_of_coins,
    )
    if len(combine_input_coin_ids) < 2:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="no_spendable_combine_coin_available",
            )
        )
        return _ledger_rows(items), 0

    try:
        result = combine_coins_with_retry(
            cloud_wallet=ctx.cloud_wallet,
            combine_kwargs={
                "number_of_coins": len(combine_input_coin_ids),
                "fee": int(ctx.program.coin_ops_combine_fee_mojos),
                "asset_id": ctx.resolved_base_asset_id,
                "largest_first": True,
                "input_coin_ids": combine_input_coin_ids,
            },
        )
    except Exception as exc:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason=f"cloud_wallet_coin_op_error:{exc}",
            )
        )
        return _ledger_rows(items), 0

    signature_request_id = str(result.get("signature_request_id", "")).strip()
    if not signature_request_id:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="combine_missing_signature_request_id",
            )
        )
        return _ledger_rows(items), 0

    items.append(
        daemon_coin_op_executed(
            op_type=op_type,
            size_base_units=size_base_units,
            op_count=op_count,
            reason="cloud_wallet_kms_combine_submitted",
            operation_id=signature_request_id,
            data={
                "requested_number_of_coins": int(requested_number_of_coins),
                "submitted_number_of_coins": int(len(combine_input_coin_ids)),
                "input_coin_cap_applied": bool(capped_number_of_coins < requested_number_of_coins),
                "input_coin_cap": int(ctx.combine_input_cap),
                "input_coin_ids": combine_input_coin_ids,
            },
        )
    )
    return _ledger_rows(items), 1

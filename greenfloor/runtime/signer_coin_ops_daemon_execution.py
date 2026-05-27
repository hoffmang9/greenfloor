"""Daemon vault-signer coin-op plan execution (coinset list + Rust mixed-split)."""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any

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
from greenfloor.runtime.cloud_wallet.coin_ops_planning import (
    CombineInputSelectionMode,
    SplitCoinPlan,
    SplitPlanningProfile,
    SplitSkipPlan,
    plan_auto_combine_inputs,
    plan_auto_split_selection,
)
from greenfloor.runtime.signer_coin_ops import (
    execute_signer_mixed_split,
    filter_signer_spendable_coins,
    list_signer_asset_coins,
)

__all__ = [
    "SignerDaemonCoinOpExecContext",
    "execute_signer_daemon_combine_plan",
    "execute_signer_daemon_split_plan",
]


@dataclass(slots=True)
class SignerDaemonCoinOpExecContext:
    program: ProgramConfig
    market: MarketConfig
    receive_address: str
    resolved_base_asset_id: str
    base_unit_mojo_multiplier: int
    combine_input_cap: int
    watched_coin_ids: set[str]
    logger: logging.Logger


def _ledger_rows(items: list[DaemonCoinOpLedgerItem]) -> list[dict[str, Any]]:
    return [item.to_dict() for item in items]


def execute_signer_daemon_split_plan(
    *,
    plan: CoinOpPlan,
    ctx: SignerDaemonCoinOpExecContext,
) -> tuple[list[dict[str, Any]], int]:
    op_type = str(plan.op_type)
    op_count = int(plan.op_count)
    size_base_units = int(plan.size_base_units)
    items: list[DaemonCoinOpLedgerItem] = []
    executed_count = 0

    if op_count <= 1:
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
            )
        )
        return _ledger_rows(items), executed_count

    required_amount = amount_per_coin_mojos * op_count
    coins = list_signer_asset_coins(
        program=ctx.program,
        receive_address=ctx.receive_address,
        asset_id=ctx.resolved_base_asset_id,
    )
    spendable = filter_signer_spendable_coins(
        coins,
        canonical_asset_id=canonical_asset_id,
        min_coin_amount_mojos=coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id),
    )
    spendable = [
        coin
        for coin in spendable
        if str(coin.get("id", coin.get("name", ""))).strip() not in ctx.watched_coin_ids
    ]
    if not spendable:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="no_spendable_split_coin_available",
            )
        )
        return _ledger_rows(items), executed_count

    selection = plan_auto_split_selection(
        candidate_spendable=spendable,
        required_amount_mojos=required_amount,
        canonical_asset_id=canonical_asset_id,
        profile=SplitPlanningProfile.DAEMON_AUTO,
        combine_input_cap=ctx.combine_input_cap,
        allow_combine_prereq=False,
    )
    if isinstance(selection, SplitSkipPlan):
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
    selected_coin_id = selection.coin_id.removeprefix("0x")
    try:
        result = execute_signer_mixed_split(
            program=ctx.program,
            receive_address=ctx.receive_address,
            asset_id=ctx.resolved_base_asset_id,
            output_amounts=[amount_per_coin_mojos] * op_count,
            coin_ids=[selected_coin_id],
            allow_sub_cat_output=False,
            no_wait=False,
        )
    except Exception as exc:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason=f"signer_coin_op_error:{exc}",
            )
        )
        return _ledger_rows(items), executed_count

    operation_id = str(result.get("operation_id", "")).strip()
    if not operation_id:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="split_missing_operation_id",
            )
        )
        return _ledger_rows(items), executed_count

    items.append(
        daemon_coin_op_executed(
            op_type=op_type,
            size_base_units=size_base_units,
            op_count=op_count,
            reason="signer_split_submitted",
            operation_id=operation_id,
        )
    )
    return _ledger_rows(items), 1


def execute_signer_daemon_combine_plan(
    *,
    plan: CoinOpPlan,
    ctx: SignerDaemonCoinOpExecContext,
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
            )
        )
        return _ledger_rows(items), 0

    coins = list_signer_asset_coins(
        program=ctx.program,
        receive_address=ctx.receive_address,
        asset_id=ctx.resolved_base_asset_id,
    )
    spendable = filter_signer_spendable_coins(
        coins,
        canonical_asset_id=canonical_asset_id,
        min_coin_amount_mojos=coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id),
    )
    spendable = [
        coin
        for coin in spendable
        if str(coin.get("id", coin.get("name", ""))).strip() not in ctx.watched_coin_ids
    ]
    combine_input_coin_ids = plan_auto_combine_inputs(
        spendable_coins=spendable,
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

    amount_by_id = {
        str(c.get("id", c.get("name", ""))).strip().lower().removeprefix("0x"): int(c.get("amount", 0))
        for c in spendable
    }
    normalized_ids = [str(value).strip().lower().removeprefix("0x") for value in combine_input_coin_ids]
    total = sum(int(amount_by_id.get(coin_id, 0)) for coin_id in normalized_ids)
    if total <= 0:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="combine_invalid_input_total",
            )
        )
        return _ledger_rows(items), 0

    output_count = max(1, min(len(normalized_ids), capped_number_of_coins))
    base = total // output_count
    remainder = total % output_count
    output_amounts = [base] * output_count
    output_amounts[-1] += remainder

    try:
        result = execute_signer_mixed_split(
            program=ctx.program,
            receive_address=ctx.receive_address,
            asset_id=ctx.resolved_base_asset_id,
            output_amounts=output_amounts,
            coin_ids=normalized_ids,
            allow_sub_cat_output=False,
            no_wait=False,
        )
    except Exception as exc:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason=f"signer_coin_op_error:{exc}",
            )
        )
        return _ledger_rows(items), 0

    operation_id = str(result.get("operation_id", "")).strip()
    if not operation_id:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason="combine_missing_operation_id",
            )
        )
        return _ledger_rows(items), 0

    items.append(
        daemon_coin_op_executed(
            op_type=op_type,
            size_base_units=size_base_units,
            op_count=op_count,
            reason="signer_combine_submitted",
            operation_id=operation_id,
        )
    )
    return _ledger_rows(items), 1

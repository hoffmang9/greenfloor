"""Daemon coin-op plan execution via unified ``CoinOpBackend`` (Cloud Wallet or signer)."""

from __future__ import annotations

import logging
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from greenfloor.config.models import (
    MarketConfig,
    ProgramConfig,
    coin_ops_execution_backend,
)
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
from greenfloor.runtime.cloud_wallet.coin_ops_models import CoinOpSelectionMode
from greenfloor.runtime.cloud_wallet.coin_ops_planning import (
    CombineInputSelectionMode,
    SplitCoinPlan,
    SplitCombinePrereqPlan,
    SplitPlanningProfile,
    SplitSkipPlan,
    plan_auto_combine_inputs,
    plan_auto_split_selection,
)

from greenfloor.runtime.coin_ops_backend import (
    CoinOpBackend,
    build_coin_op_backend,
    resolve_coin_op_base_asset_id,
)

__all__ = [
    "DaemonCoinOpExecContext",
    "execute_daemon_combine_plan",
    "execute_daemon_split_plan",
    "execute_managed_coin_op_plans",
]


@dataclass(slots=True)
class DaemonCoinOpExecContext:
    backend: CoinOpBackend
    market: MarketConfig
    program: ProgramConfig
    resolved_base_asset_id: str
    base_unit_mojo_multiplier: int
    combine_input_cap: int
    watched_coin_ids: set[str]
    logger: logging.Logger

    def list_asset_coins(self) -> list[dict[str, Any]]:
        return self.backend.list_asset_scoped_coins()

    def filter_spendable_coins(self, coins: list[dict[str, Any]]) -> list[dict[str, Any]]:
        canonical_asset_id = str(self.market.base_asset).strip()
        spendable = self.backend.filter_spendable(
            coins,
            canonical_asset_id=canonical_asset_id,
            min_coin_amount_mojos=coin_op_min_amount_mojos(
                canonical_asset_id=canonical_asset_id
            ),
            mode=CoinOpSelectionMode.DAEMON,
        )
        return spendable


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
        combine_result = ctx.backend.combine_coins(
            number_of_coins=len(prereq.input_coin_ids),
            fee_mojos=int(ctx.program.coin_ops_combine_fee_mojos),
            input_coin_ids=prereq.input_coin_ids,
            target_amount=prereq.target_amount,
        )
    except Exception as exc:
        return _ledger_rows(
            [
                daemon_coin_op_skipped(
                    op_type=op_type,
                    size_base_units=size_base_units,
                    op_count=op_count,
                    reason=(
                        f"{ctx.backend.scope.coin_op_error_prefix()}:{exc}:combine_for_split_prereq"
                    ),
                )
            ]
        ), 0
    combine_sig_id = str(
        combine_result.get("signature_request_id") or combine_result.get("operation_id", "")
    ).strip()
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
                reason=ctx.backend.scope.combine_prereq_submitted_reason(
                    exact_match=bool(prereq.exact_match)
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
    coins = ctx.list_asset_coins()
    if not ctx.filter_spendable_coins(coins):
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
        fresh = ctx.list_asset_coins()
        candidate_spendable = [
            coin
            for coin in ctx.filter_spendable_coins(fresh)
            if str(coin.get("id", coin.get("name", ""))).strip() not in attempted_coin_ids
            and str(coin.get("id", coin.get("name", ""))).strip() not in ctx.watched_coin_ids
        ]
        selection = plan_auto_split_selection(
            candidate_spendable=candidate_spendable,
            required_amount_mojos=required_amount,
            canonical_asset_id=canonical_asset_id,
            profile=SplitPlanningProfile.DAEMON_AUTO,
            combine_input_cap=ctx.combine_input_cap,
            allow_combine_prereq=(
                attempt_index == 0 and ctx.backend.scope.allows_daemon_split_combine_prereq
            ),
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
            result = ctx.backend.split_coins(
                coin_ids=[selected_coin_id],
                amount_per_coin=amount_per_coin_mojos,
                number_of_coins=op_count,
                fee_mojos=int(ctx.program.coin_ops_split_fee_mojos),
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
                        f"{ctx.backend.scope.coin_op_error_prefix()}:{exc}:"
                        f"selected_coin_id={selected_coin_id}"
                    ),
                )
            )
            return _ledger_rows(items), executed_count

        operation_id = str(
            result.get("signature_request_id") or result.get("operation_id", "")
        ).strip()
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
                reason=ctx.backend.scope.split_submitted_reason(),
                operation_id=operation_id,
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
        spendable_coins=ctx.filter_spendable_coins(ctx.list_asset_coins()),
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
        result = ctx.backend.combine_coins(
            number_of_coins=len(combine_input_coin_ids),
            fee_mojos=int(ctx.program.coin_ops_combine_fee_mojos),
            input_coin_ids=combine_input_coin_ids,
        )
    except Exception as exc:
        items.append(
            daemon_coin_op_skipped(
                op_type=op_type,
                size_base_units=size_base_units,
                op_count=op_count,
                reason=f"coin_op_error:{exc}",
            )
        )
        return _ledger_rows(items), 0

    operation_id = str(
        result.get("signature_request_id") or result.get("operation_id", "")
    ).strip()
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
            reason=ctx.backend.scope.combine_submitted_reason(),
            operation_id=operation_id,
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


def _skip_all_plans(
    *,
    program: ProgramConfig,
    plans: list[CoinOpPlan],
    reason: str,
    status: str,
    signer_selection: Any,
) -> dict[str, Any]:
    return {
        "dry_run": bool(program.runtime_dry_run),
        "planned_count": len(plans),
        "executed_count": 0,
        "status": status,
        "signer_selection": {
            "selected_source": "signer_registry",
            "key_id": str(getattr(signer_selection, "key_id", "")).strip(),
            "network": str(getattr(program, "app_network", "")).strip(),
        },
        "items": [
            {
                "op_type": plan.op_type,
                "size_base_units": plan.size_base_units,
                "op_count": plan.op_count,
                "status": "skipped",
                "reason": reason,
                "operation_id": None,
            }
            for plan in plans
        ],
    }


def execute_managed_coin_op_plans(
    *,
    market: MarketConfig,
    program: ProgramConfig,
    plans: list[CoinOpPlan],
    signer_selection: Any,
    base_unit_mojo_multiplier: int,
    combine_input_cap: int,
    watched_coin_ids: set[str],
    logger: logging.Logger,
    cloud_wallet_configured: bool,
    cloud_wallet_base_asset_resolver: Callable[[], str] | None = None,
    deps: Any = None,
) -> dict[str, Any]:
    backend_name = coin_ops_execution_backend(program)
    if backend_name == "cloud_wallet":
        if not cloud_wallet_configured:
            return _skip_all_plans(
                program=program,
                plans=plans,
                reason="cloud_wallet_required_for_coin_ops",
                status="skipped",
                signer_selection=signer_selection,
            )
        if not str(getattr(program, "cloud_wallet_kms_key_id", "")).strip():
            return _skip_all_plans(
                program=program,
                plans=plans,
                reason="cloud_wallet_kms_required_for_coin_ops",
                status="skipped",
                signer_selection=signer_selection,
            )

    try:
        if backend_name == "signer" and not str(market.receive_address).strip():
            raise ValueError("signer_coin_ops_missing_receive_address")
        if backend_name == "signer":
            resolved_base_asset_id = resolve_coin_op_base_asset_id(
                program=program, market=market, deps=deps
            )
        elif cloud_wallet_base_asset_resolver is not None:
            resolved_base_asset_id = str(cloud_wallet_base_asset_resolver()).strip()
        else:
            resolved_base_asset_id = resolve_coin_op_base_asset_id(
                program=program, market=market, deps=deps
            )
        backend = build_coin_op_backend(
            program=program,
            market=market,
            selected_venue=None,
            resolved_asset_id=resolved_base_asset_id,
            deps=deps,
        )
    except ValueError as exc:
        return _skip_all_plans(
            program=program,
            plans=plans,
            reason=str(exc),
            status="skipped",
            signer_selection=signer_selection,
        )

    exec_ctx = DaemonCoinOpExecContext(
        backend=backend,
        market=market,
        program=program,
        resolved_base_asset_id=resolved_base_asset_id,
        base_unit_mojo_multiplier=base_unit_mojo_multiplier,
        combine_input_cap=combine_input_cap,
        watched_coin_ids=watched_coin_ids,
        logger=logger,
    )
    items: list[dict[str, Any]] = []
    executed_count = 0
    scope = backend.scope
    dry_run_reason = scope.dry_run_reason()
    execution_status = "signer" if backend_name == "signer" else "cloud_wallet_kms"

    for plan in plans:
        op_type = str(plan.op_type)
        op_count = int(plan.op_count)
        size_base_units = int(plan.size_base_units)
        if op_count <= 0 or size_base_units <= 0:
            items.append(
                {
                    "op_type": op_type,
                    "size_base_units": size_base_units,
                    "op_count": op_count,
                    "status": "skipped",
                    "reason": "invalid_plan",
                    "operation_id": None,
                }
            )
            continue
        if bool(program.runtime_dry_run):
            items.append(
                {
                    "op_type": op_type,
                    "size_base_units": size_base_units,
                    "op_count": op_count,
                    "status": "planned",
                    "reason": dry_run_reason,
                    "operation_id": None,
                }
            )
            continue
        try:
            if op_type == "split":
                split_items, split_executed = execute_daemon_split_plan(plan=plan, ctx=exec_ctx)
                items.extend(split_items)
                executed_count += split_executed
                continue
            if op_type == "combine":
                combine_items, combine_executed = execute_daemon_combine_plan(
                    plan=plan, ctx=exec_ctx
                )
                items.extend(combine_items)
                executed_count += combine_executed
                continue
            items.append(
                {
                    "op_type": op_type,
                    "size_base_units": size_base_units,
                    "op_count": op_count,
                    "status": "skipped",
                    "reason": "invalid_plan",
                    "operation_id": None,
                }
            )
        except Exception as exc:
            items.append(
                {
                    "op_type": op_type,
                    "size_base_units": size_base_units,
                    "op_count": op_count,
                    "status": "skipped",
                    "reason": f"{scope.coin_op_error_prefix()}:{exc}",
                    "operation_id": None,
                }
            )

    return {
        "dry_run": bool(program.runtime_dry_run),
        "planned_count": len(plans),
        "executed_count": executed_count,
        "status": execution_status,
        "signer_selection": {
            "selected_source": "signer_registry",
            "key_id": str(getattr(signer_selection, "key_id", "")).strip(),
            "network": str(getattr(program, "app_network", "")).strip(),
        },
        "items": items,
    }

"""Coin split/combine iteration step bodies shared by CLI commands."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.config.models import MarketConfig
from greenfloor.core.coin_ops_policy import coin_meets_coin_op_min_amount
from greenfloor.runtime.cloud_wallet.coin_op_errors import (
    coin_combine_asset_mismatch_error_payload,
    coin_combine_insufficient_coins_error_payload,
    coin_split_lockup_guardrail_error_payload,
    coin_split_no_spendable_error_payload,
)
from greenfloor.runtime.cloud_wallet.coin_ops_execution import combine_coins_with_retry
from greenfloor.runtime.cloud_wallet.coin_ops_models import (
    CoinOpSelectionMode,
    CombineDenominationTarget,
    SplitDenominationTarget,
    filter_spendable_for_coin_ops,
)
from greenfloor.runtime.cloud_wallet.coin_ops_planning import (
    CombineInputSelectionMode,
    SplitCoinPlan,
    SplitPlanningProfile,
    SplitSkipPlan,
    plan_auto_combine_inputs,
    plan_auto_split_selection,
)
from greenfloor.runtime.cloud_wallet.coin_ops_runtime import (
    CoinOpIterationEarlyExit,
    CoinOpIterationExecuteResult,
    CoinOpIterationNeedsConfirmation,
    CoinOpIterationSkipLoop,
    evaluate_coin_split_gate,
)
from greenfloor.runtime.cloud_wallet.coins import (
    classify_resolved_coin_ids_by_asset,
    resolve_coin_global_ids,
)


@dataclass(slots=True)
class CoinSplitStepParams:
    wallet: CloudWalletAdapter
    market: MarketConfig
    selected_venue: str | None
    resolved_asset_id: str
    explicit_coin_ids: list[str]
    amount_per_coin: int
    number_of_coins: int
    fee_mojos: int
    denomination_target: SplitDenominationTarget | None
    min_coin_amount_mojos: int
    allow_lock_all_spendable: bool
    force_split_when_ready: bool


@dataclass(slots=True)
class CoinSplitStepResult:
    step: (
        CoinOpIterationExecuteResult
        | CoinOpIterationEarlyExit
        | CoinOpIterationSkipLoop
        | CoinOpIterationNeedsConfirmation
    )
    split_gate: dict[str, int | bool | str] | None = None


def run_coin_split_step(
    *,
    params: CoinSplitStepParams,
    wallet_coins: list[dict[str, Any]],
) -> CoinSplitStepResult:
    asset_scoped_coins = params.wallet.list_coins(
        asset_id=params.resolved_asset_id,
        include_pending=True,
    )
    canonical_asset_id = str(params.market.base_asset)
    spendable_scoped = filter_spendable_for_coin_ops(
        coins=asset_scoped_coins,
        wallet=params.wallet,
        resolved_asset_id=params.resolved_asset_id,
        canonical_asset_id=canonical_asset_id,
        mode=CoinOpSelectionMode.CLI,
    )
    spendable_asset_coin_ids = {
        str(c.get("id", "")).strip() for c in spendable_scoped if str(c.get("id", "")).strip()
    }
    split_gate: dict[str, int | bool | str] | None = None
    if params.denomination_target is not None:
        split_gate = evaluate_coin_split_gate(
            asset_scoped_coins=asset_scoped_coins,
            resolved_asset_id=params.resolved_asset_id,
            size_base_units=params.denomination_target.size_base_units,
            required_count=params.denomination_target.required_count,
        )
        if bool(split_gate["ready"]) and not params.force_split_when_ready:
            return CoinSplitStepResult(
                step=CoinOpIterationNeedsConfirmation(
                    message=(
                        "split gate is already satisfied "
                        "(target+buffer met and reserve available). Force another split anyway?"
                    ),
                    override="force_split_when_ready",
                    decline_step=CoinOpIterationSkipLoop(stop_reason="ready"),
                ),
                split_gate=split_gate,
            )
    if params.explicit_coin_ids:
        resolved_coin_ids, unresolved_coin_ids = resolve_coin_global_ids(
            wallet_coins, params.explicit_coin_ids
        )
        if unresolved_coin_ids:
            return CoinSplitStepResult(
                step=CoinOpIterationEarlyExit(
                    return_code=2,
                    unresolved_coin_ids=unresolved_coin_ids,
                ),
                split_gate=split_gate,
            )
    else:
        spendable_asset_coins = [
            c
            for c in spendable_scoped
            if coin_meets_coin_op_min_amount(c, canonical_asset_id=canonical_asset_id)
        ]
        if not spendable_asset_coins:
            return CoinSplitStepResult(
                step=CoinOpIterationEarlyExit(
                    return_code=2,
                    error_payload=coin_split_no_spendable_error_payload(
                        market=params.market,
                        selected_venue=params.selected_venue,
                        wallet=params.wallet,
                        canonical_asset_id=canonical_asset_id,
                        resolved_asset_id=params.resolved_asset_id,
                        min_coin_amount_mojos=params.min_coin_amount_mojos,
                    ),
                ),
                split_gate=split_gate,
            )
        selection = plan_auto_split_selection(
            candidate_spendable=spendable_asset_coins,
            required_amount_mojos=params.amount_per_coin * params.number_of_coins,
            canonical_asset_id=canonical_asset_id,
            profile=SplitPlanningProfile.CLI_AUTO,
            combine_input_cap=0,
        )
        if isinstance(selection, SplitSkipPlan):
            raise RuntimeError("coin_split_failed:missing_selected_coin_id")
        assert isinstance(selection, SplitCoinPlan)
        resolved_coin_ids = [selection.coin_id]

    if (
        not params.allow_lock_all_spendable
        and spendable_asset_coin_ids
        and set(resolved_coin_ids) >= spendable_asset_coin_ids
    ):
        return CoinSplitStepResult(
            step=CoinOpIterationNeedsConfirmation(
                message=(
                    "coin-split would lock all currently spendable coins for this asset. "
                    "Override and continue?"
                ),
                override="allow_lock_all_spendable",
                decline_step=CoinOpIterationEarlyExit(
                    return_code=2,
                    error_payload=coin_split_lockup_guardrail_error_payload(
                        market=params.market,
                        selected_venue=params.selected_venue,
                        wallet=params.wallet,
                        resolved_asset_id=params.resolved_asset_id,
                        spendable_asset_coin_ids=spendable_asset_coin_ids,
                        selected_coin_ids=resolved_coin_ids,
                    ),
                ),
            ),
            split_gate=split_gate,
        )

    split_result = params.wallet.split_coins(
        coin_ids=resolved_coin_ids,
        amount_per_coin=params.amount_per_coin,
        number_of_coins=params.number_of_coins,
        fee=params.fee_mojos,
    )
    signature_request_id = split_result["signature_request_id"]
    if not signature_request_id:
        raise RuntimeError("coin_split_failed:missing_signature_request_id")

    readiness_kwargs: dict[str, int] = {}
    if params.denomination_target is not None:
        readiness_kwargs = params.denomination_target.split_readiness_kwargs()
    return CoinSplitStepResult(
        step=CoinOpIterationExecuteResult(
            signature_request_id=signature_request_id,
            initial_signature_state=str(split_result.get("status", "UNKNOWN")),
            readiness_kwargs=readiness_kwargs,
        ),
        split_gate=split_gate,
    )


@dataclass(slots=True)
class CoinCombineStepParams:
    wallet: CloudWalletAdapter
    market: MarketConfig
    selected_venue: str | None
    resolved_asset_id: str
    combine_canonical_asset_id: str
    explicit_coin_ids: list[str]
    number_of_coins: int
    fee_mojos: int
    denomination_target: CombineDenominationTarget | None
    min_coin_amount_mojos: int


@dataclass(slots=True)
class CoinCombineStepResult:
    step: CoinOpIterationExecuteResult | CoinOpIterationEarlyExit
    split_gate: dict[str, int | bool | str] | None = None


def run_coin_combine_step(
    *,
    params: CoinCombineStepParams,
    wallet_coins: list[dict[str, Any]],
) -> CoinCombineStepResult:
    resolved_input_coin_ids: list[str] | None = None
    if params.explicit_coin_ids:
        resolved_input_coin_ids, unresolved_coin_ids = resolve_coin_global_ids(
            wallet_coins, params.explicit_coin_ids
        )
        if unresolved_coin_ids:
            return CoinCombineStepResult(
                step=CoinOpIterationEarlyExit(
                    return_code=2,
                    unresolved_coin_ids=unresolved_coin_ids,
                ),
            )
        if params.number_of_coins != len(resolved_input_coin_ids):
            raise ValueError(
                "when --coin-id is provided, --input-coin-count must match the number of --coin-id values"
            )
        unresolved_coin_ids, mismatched_coin_ids = classify_resolved_coin_ids_by_asset(
            wallet_coins=wallet_coins,
            resolved_coin_ids=resolved_input_coin_ids,
            expected_asset_id=params.resolved_asset_id,
        )
        if unresolved_coin_ids:
            return CoinCombineStepResult(
                step=CoinOpIterationEarlyExit(
                    return_code=2,
                    unresolved_coin_ids=unresolved_coin_ids,
                ),
            )
        if mismatched_coin_ids:
            return CoinCombineStepResult(
                step=CoinOpIterationEarlyExit(
                    return_code=2,
                    error_payload=coin_combine_asset_mismatch_error_payload(
                        market=params.market,
                        selected_venue=params.selected_venue,
                        wallet=params.wallet,
                        resolved_asset_id=params.resolved_asset_id,
                        mismatched_coin_ids=mismatched_coin_ids,
                    ),
                ),
            )
    elif params.min_coin_amount_mojos > 0:
        asset_scoped_coins = params.wallet.list_coins(
            asset_id=params.resolved_asset_id,
            include_pending=True,
        )
        spendable_scoped = filter_spendable_for_coin_ops(
            coins=asset_scoped_coins,
            wallet=params.wallet,
            resolved_asset_id=params.resolved_asset_id,
            canonical_asset_id=params.combine_canonical_asset_id,
            mode=CoinOpSelectionMode.CLI,
            verify_direct_spendable_lookup=True,
        )
        resolved_input_coin_ids = plan_auto_combine_inputs(
            spendable_coins=spendable_scoped,
            number_of_coins=params.number_of_coins,
            selection_mode=CombineInputSelectionMode.LARGEST_BY_AMOUNT,
        )
        if len(resolved_input_coin_ids) < params.number_of_coins:
            return CoinCombineStepResult(
                step=CoinOpIterationEarlyExit(
                    return_code=2,
                    error_payload=coin_combine_insufficient_coins_error_payload(
                        market=params.market,
                        selected_venue=params.selected_venue,
                        wallet=params.wallet,
                        combine_canonical_asset_id=params.combine_canonical_asset_id,
                        resolved_asset_id=params.resolved_asset_id,
                        required_coin_count=int(params.number_of_coins),
                        eligible_coin_count=len(resolved_input_coin_ids),
                        min_coin_amount_mojos=params.min_coin_amount_mojos,
                    ),
                ),
            )

    combine_result = combine_coins_with_retry(
        cloud_wallet=params.wallet,
        combine_kwargs={
            "number_of_coins": params.number_of_coins,
            "fee": params.fee_mojos,
            "asset_id": params.resolved_asset_id,
            "largest_first": True,
            "input_coin_ids": resolved_input_coin_ids,
        },
    )
    signature_request_id = combine_result["signature_request_id"]
    if not signature_request_id:
        raise RuntimeError("coin_combine_failed:missing_signature_request_id")

    readiness_kwargs: dict[str, int] = {}
    if params.denomination_target is not None:
        readiness_kwargs = params.denomination_target.combine_readiness_kwargs()
    return CoinCombineStepResult(
        step=CoinOpIterationExecuteResult(
            signature_request_id=signature_request_id,
            initial_signature_state=str(combine_result.get("status", "UNKNOWN")),
            readiness_kwargs=readiness_kwargs,
        ),
    )

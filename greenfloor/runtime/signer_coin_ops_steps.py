"""Signer-backed coin split/combine step bodies (coinset list + Rust mixed-split)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.runtime.cloud_wallet.coin_op_errors import (
    coin_combine_insufficient_coins_error_payload,
    coin_split_lockup_guardrail_error_payload,
    coin_split_no_spendable_error_payload,
)
from greenfloor.runtime.cloud_wallet.coin_ops_models import (
    CombineDenominationTarget,
    SplitDenominationTarget,
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
from greenfloor.runtime.signer_coin_ops import (
    execute_signer_mixed_split,
    filter_signer_spendable_coins,
    list_signer_asset_coins,
    resolve_hex_coin_ids,
)


@dataclass(slots=True)
class SignerCoinSplitStepParams:
    program: ProgramConfig
    market: MarketConfig
    selected_venue: str | None
    receive_address: str
    resolved_asset_id: str
    explicit_coin_ids: list[str]
    amount_per_coin: int
    number_of_coins: int
    denomination_target: SplitDenominationTarget | None
    min_coin_amount_mojos: int
    allow_lock_all_spendable: bool
    force_split_when_ready: bool
    no_wait: bool


@dataclass(slots=True)
class SignerCoinCombineStepParams:
    program: ProgramConfig
    market: MarketConfig
    selected_venue: str | None
    receive_address: str
    resolved_asset_id: str
    combine_canonical_asset_id: str
    explicit_coin_ids: list[str]
    number_of_coins: int
    denomination_target: CombineDenominationTarget | None
    min_coin_amount_mojos: int
    no_wait: bool


@dataclass(slots=True)
class SignerCoinOpStepResult:
    step: (
        CoinOpIterationExecuteResult
        | CoinOpIterationEarlyExit
        | CoinOpIterationSkipLoop
        | CoinOpIterationNeedsConfirmation
    )
    split_gate: dict[str, int | bool | str] | None = None


def _signer_wallet_stub(*, resolved_asset_id: str) -> Any:
    """Minimal object for shared planning helpers that expect a wallet with vault_id."""

    class _Stub:
        vault_id = "signer"

        def list_coins(self, *, asset_id: str, include_pending: bool = True) -> list[dict]:
            _ = asset_id, include_pending
            return []

    return _Stub()


def run_signer_coin_split_step(
    *,
    params: SignerCoinSplitStepParams,
    asset_scoped_coins: list[dict[str, Any]],
) -> SignerCoinOpStepResult:
    canonical_asset_id = str(params.market.base_asset)
    spendable_scoped = filter_signer_spendable_coins(
        asset_scoped_coins,
        canonical_asset_id=canonical_asset_id,
        min_coin_amount_mojos=params.min_coin_amount_mojos,
    )
    spendable_asset_coin_ids = {
        str(c.get("id", c.get("name", ""))).strip() for c in spendable_scoped if c.get("id") or c.get("name")
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
            return SignerCoinOpStepResult(
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

    wallet_stub = _signer_wallet_stub(resolved_asset_id=params.resolved_asset_id)
    if params.explicit_coin_ids:
        resolved_coin_ids, unresolved_coin_ids = resolve_hex_coin_ids(
            asset_scoped_coins, params.explicit_coin_ids
        )
        if unresolved_coin_ids:
            return SignerCoinOpStepResult(
                step=CoinOpIterationEarlyExit(
                    return_code=2,
                    unresolved_coin_ids=unresolved_coin_ids,
                ),
                split_gate=split_gate,
            )
    else:
        if not spendable_scoped:
            return SignerCoinOpStepResult(
                step=CoinOpIterationEarlyExit(
                    return_code=2,
                    error_payload=coin_split_no_spendable_error_payload(
                        market=params.market,
                        selected_venue=params.selected_venue,
                        wallet=wallet_stub,
                        canonical_asset_id=canonical_asset_id,
                        resolved_asset_id=params.resolved_asset_id,
                        min_coin_amount_mojos=params.min_coin_amount_mojos,
                    ),
                ),
                split_gate=split_gate,
            )
        selection = plan_auto_split_selection(
            candidate_spendable=spendable_scoped,
            required_amount_mojos=params.amount_per_coin * params.number_of_coins,
            canonical_asset_id=canonical_asset_id,
            profile=SplitPlanningProfile.CLI_AUTO,
            combine_input_cap=0,
        )
        if isinstance(selection, SplitSkipPlan):
            raise RuntimeError("coin_split_failed:missing_selected_coin_id")
        assert isinstance(selection, SplitCoinPlan)
        resolved_coin_ids = [selection.coin_id.removeprefix("0x")]

    if (
        not params.allow_lock_all_spendable
        and spendable_asset_coin_ids
        and set(resolved_coin_ids) >= {c.removeprefix("0x") for c in spendable_asset_coin_ids}
    ):
        return SignerCoinOpStepResult(
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
                        wallet=wallet_stub,
                        resolved_asset_id=params.resolved_asset_id,
                        spendable_asset_coin_ids=list(spendable_asset_coin_ids),
                        selected_coin_ids=resolved_coin_ids,
                    ),
                ),
            ),
            split_gate=split_gate,
        )

    output_amounts = [int(params.amount_per_coin)] * int(params.number_of_coins)
    exec_result = execute_signer_mixed_split(
        program=params.program,
        receive_address=params.receive_address,
        asset_id=params.resolved_asset_id,
        output_amounts=output_amounts,
        coin_ids=resolved_coin_ids,
        allow_sub_cat_output=False,
        no_wait=params.no_wait,
        initial_coin_ids=spendable_asset_coin_ids,
    )
    operation_id = str(exec_result.get("operation_id", "")).strip()
    if not operation_id:
        raise RuntimeError("coin_split_failed:missing_operation_id")

    readiness_kwargs: dict[str, int] = {}
    if params.denomination_target is not None:
        readiness_kwargs = params.denomination_target.split_readiness_kwargs()
    return SignerCoinOpStepResult(
        step=CoinOpIterationExecuteResult(
            signature_request_id=operation_id,
            initial_signature_state=str(exec_result.get("broadcast_status", "submitted")),
            readiness_kwargs=readiness_kwargs,
        ),
        split_gate=split_gate,
    )


def run_signer_coin_combine_step(
    *,
    params: SignerCoinCombineStepParams,
    asset_scoped_coins: list[dict[str, Any]],
) -> SignerCoinOpStepResult:
    wallet_stub = _signer_wallet_stub(resolved_asset_id=params.resolved_asset_id)
    resolved_input_coin_ids: list[str] | None = None
    if params.explicit_coin_ids:
        resolved_input_coin_ids, unresolved_coin_ids = resolve_hex_coin_ids(
            asset_scoped_coins, params.explicit_coin_ids
        )
        if unresolved_coin_ids:
            return SignerCoinOpStepResult(
                step=CoinOpIterationEarlyExit(
                    return_code=2,
                    unresolved_coin_ids=unresolved_coin_ids,
                ),
            )
        if params.number_of_coins != len(resolved_input_coin_ids):
            raise ValueError(
                "when --coin-id is provided, --input-coin-count must match the number of --coin-id values"
            )
    elif params.min_coin_amount_mojos > 0:
        spendable_scoped = filter_signer_spendable_coins(
            asset_scoped_coins,
            canonical_asset_id=params.combine_canonical_asset_id,
            min_coin_amount_mojos=params.min_coin_amount_mojos,
        )
        resolved_input_coin_ids = plan_auto_combine_inputs(
            spendable_coins=spendable_scoped,
            number_of_coins=params.number_of_coins,
            selection_mode=CombineInputSelectionMode.LARGEST_BY_AMOUNT,
        )
        if len(resolved_input_coin_ids) < params.number_of_coins:
            return SignerCoinOpStepResult(
                step=CoinOpIterationEarlyExit(
                    return_code=2,
                    error_payload=coin_combine_insufficient_coins_error_payload(
                        market=params.market,
                        selected_venue=params.selected_venue,
                        wallet=wallet_stub,
                        combine_canonical_asset_id=params.combine_canonical_asset_id,
                        resolved_asset_id=params.resolved_asset_id,
                        required_coin_count=int(params.number_of_coins),
                        eligible_coin_count=len(resolved_input_coin_ids),
                        min_coin_amount_mojos=params.min_coin_amount_mojos,
                    ),
                ),
            )
    else:
        raise RuntimeError("coin_combine_failed:missing_input_selection")

    assert resolved_input_coin_ids is not None
    amount_by_id = {
        str(c.get("id", c.get("name", ""))).strip().lower().removeprefix("0x"): int(c.get("amount", 0))
        for c in asset_scoped_coins
    }
    total: int = 0
    normalized_ids: list[str] = []
    for coin_id in resolved_input_coin_ids:
        key = str(coin_id).strip().lower().removeprefix("0x")
        normalized_ids.append(key)
        total += int(amount_by_id.get(key, 0))
    if total <= 0:
        raise RuntimeError("coin_combine_failed:invalid_input_total")

    output_count = max(1, int(params.number_of_coins))
    base = total // output_count
    remainder = total % output_count
    output_amounts = [base] * output_count
    output_amounts[-1] += remainder

    existing_ids = {
        str(c.get("id", c.get("name", ""))).strip() for c in asset_scoped_coins if c.get("id") or c.get("name")
    }
    exec_result = execute_signer_mixed_split(
        program=params.program,
        receive_address=params.receive_address,
        asset_id=params.resolved_asset_id,
        output_amounts=output_amounts,
        coin_ids=normalized_ids,
        allow_sub_cat_output=False,
        no_wait=params.no_wait,
        initial_coin_ids=existing_ids,
    )
    operation_id = str(exec_result.get("operation_id", "")).strip()
    if not operation_id:
        raise RuntimeError("coin_combine_failed:missing_operation_id")

    readiness_kwargs: dict[str, int] = {}
    if params.denomination_target is not None:
        readiness_kwargs = params.denomination_target.combine_readiness_kwargs()
    return SignerCoinOpStepResult(
        step=CoinOpIterationExecuteResult(
            signature_request_id=operation_id,
            initial_signature_state=str(exec_result.get("broadcast_status", "submitted")),
            readiness_kwargs=readiness_kwargs,
        ),
    )


def list_asset_scoped_coins_for_signer_step(
    *,
    program: ProgramConfig,
    receive_address: str,
    resolved_asset_id: str,
) -> list[dict[str, Any]]:
    return list_signer_asset_coins(
        program=program,
        receive_address=receive_address,
        asset_id=resolved_asset_id,
    )

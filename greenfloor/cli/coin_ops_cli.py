"""CLI orchestration for coin-split and coin-combine commands."""

from __future__ import annotations

import math
from collections.abc import Callable
from pathlib import Path

from greenfloor.cli.prompts import prompt_yes_no
from greenfloor.core.coin_ops_policy import coin_op_min_amount_mojos
from greenfloor.runtime.cloud_wallet.adapter import format_json_output
from greenfloor.runtime.cloud_wallet.coin_op_errors import coin_op_unresolved_error_payload
from greenfloor.runtime.cloud_wallet.coin_ops_models import (
    CombineDenominationTarget,
    SplitDenominationTarget,
)
from greenfloor.runtime.cloud_wallet.coin_ops_runtime import (
    CoinOpIterationNeedsConfirmation,
    CoinOpLoopResult,
    CoinOpSetup,
    CoinOpStepOutcome,
    coin_op_result_payload,
    coin_op_setup,
    resolve_market_denomination_entry,
    run_coin_op_iteration_loop,
)
from greenfloor.runtime.cloud_wallet.coin_ops_steps import (
    CoinCombineStepParams,
    CoinSplitStepParams,
    run_coin_combine_step,
    run_coin_split_step,
)


def _finish_coin_op_cli(
    *,
    setup: CoinOpSetup,
    loop_result: CoinOpLoopResult,
    until_ready: bool,
    success_payload: dict[str, object],
) -> int:
    if loop_result.early_return_code is not None:
        if loop_result.error_payload is not None:
            print(format_json_output(loop_result.error_payload))
        elif loop_result.unresolved_coin_ids:
            print(
                format_json_output(
                    coin_op_unresolved_error_payload(
                        market=setup.market,
                        selected_venue=setup.selected_venue,
                        wallet=setup.wallet,
                        unresolved_coin_ids=loop_result.unresolved_coin_ids,
                    )
                )
            )
        return loop_result.early_return_code

    final_readiness = loop_result.final_readiness
    print(format_json_output(success_payload))
    if until_ready and final_readiness is not None and not bool(final_readiness["ready"]):
        return 2
    return 0


def _resolve_split_targets(
    *,
    market,
    amount_per_coin: int,
    number_of_coins: int,
    size_base_units: int | None,
) -> tuple[int, int, SplitDenominationTarget | None]:
    resolved_amount = amount_per_coin
    resolved_count = number_of_coins
    denomination_target: SplitDenominationTarget | None = None
    if size_base_units is not None and int(size_base_units) > 0:
        entry = resolve_market_denomination_entry(market, size_base_units=int(size_base_units))
        target = SplitDenominationTarget.from_ladder_entry(entry)
        if resolved_amount <= 0:
            resolved_amount = target.size_base_units
        elif resolved_amount != target.size_base_units:
            raise ValueError(
                "amount_per_coin must match market ladder size when --size-base-units is set"
            )
        if resolved_count <= 0:
            resolved_count = target.required_count
        elif resolved_count != target.required_count:
            raise ValueError(
                "number_of_coins must match market ladder target+buffer when --size-base-units is set"
            )
        denomination_target = target
    elif resolved_amount <= 0:
        raise ValueError("amount_per_coin must be positive")
    elif resolved_count <= 0:
        raise ValueError("number_of_coins must be positive")
    return resolved_amount, resolved_count, denomination_target


def _resolve_combine_targets(
    *,
    market,
    number_of_coins: int,
    size_base_units: int | None,
    requested_asset_id: str | None,
) -> tuple[int, CombineDenominationTarget | None, str]:
    resolved_count = number_of_coins
    denomination_target: CombineDenominationTarget | None = None
    if size_base_units is not None and int(size_base_units) > 0:
        entry = resolve_market_denomination_entry(market, size_base_units=int(size_base_units))
        threshold = max(
            2,
            int(math.ceil(int(entry.target_count) * float(entry.combine_when_excess_factor))),
        )
        if resolved_count <= 0:
            resolved_count = threshold
        elif resolved_count != threshold:
            raise ValueError(
                "number_of_coins must match market ladder combine threshold when --size-base-units is set"
            )
        denomination_target = CombineDenominationTarget.from_ladder_entry(
            entry, threshold=threshold
        )
    if resolved_count <= 1:
        raise ValueError("number_of_coins must be > 1")
    combine_canonical_asset_id = requested_asset_id or str(market.base_asset)
    return resolved_count, denomination_target, combine_canonical_asset_id


def _build_split_run_step(
    *,
    step_params: CoinSplitStepParams,
    prompt_for_override: bool | None,
) -> Callable[[int, list[dict], set[str]], CoinOpStepOutcome]:
    def run_step(
        iteration: int,
        wallet_coins: list[dict],
        existing_coin_ids: set[str],
    ) -> CoinOpStepOutcome:
        _ = iteration, existing_coin_ids
        while True:
            step_result = run_coin_split_step(params=step_params, wallet_coins=wallet_coins)
            step = step_result.step
            if isinstance(step, CoinOpIterationNeedsConfirmation):
                if prompt_yes_no(step.message, prompt_for_override=prompt_for_override):
                    if step.override == "force_split_when_ready":
                        step_params.force_split_when_ready = True
                    elif step.override == "allow_lock_all_spendable":
                        step_params.allow_lock_all_spendable = True
                    continue
                return CoinOpStepOutcome(step=step.decline_step, split_gate=step_result.split_gate)
            return CoinOpStepOutcome(step=step, split_gate=step_result.split_gate)

    return run_step


def execute_split_cli(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    network: str,
    market_id: str | None,
    pair: str | None,
    venue: str | None,
    coin_ids: list[str],
    amount_per_coin: int,
    number_of_coins: int,
    no_wait: bool,
    size_base_units: int | None,
    until_ready: bool,
    max_iterations: int,
    allow_lock_all_spendable: bool,
    force_split_when_ready: bool,
    prompt_for_override: bool | None,
) -> int:
    setup_result = coin_op_setup(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        network=network,
        market_id=market_id,
        pair=pair,
        venue=venue,
    )
    if setup_result.error_payload is not None:
        print(format_json_output(setup_result.error_payload))
        return 2
    setup = setup_result.setup
    assert setup is not None

    resolved_amount, resolved_count, denomination_target = _resolve_split_targets(
        market=setup.market,
        amount_per_coin=amount_per_coin,
        number_of_coins=number_of_coins,
        size_base_units=size_base_units,
    )
    step_params = CoinSplitStepParams(
        wallet=setup.wallet,
        market=setup.market,
        selected_venue=setup.selected_venue,
        resolved_asset_id=setup.resolved_asset_id,
        explicit_coin_ids=coin_ids,
        amount_per_coin=resolved_amount,
        number_of_coins=resolved_count,
        fee_mojos=setup.fee_mojos,
        denomination_target=denomination_target,
        min_coin_amount_mojos=coin_op_min_amount_mojos(
            canonical_asset_id=str(setup.market.base_asset)
        ),
        allow_lock_all_spendable=allow_lock_all_spendable,
        force_split_when_ready=force_split_when_ready,
    )
    loop_result = run_coin_op_iteration_loop(
        wallet=setup.wallet,
        network=network,
        no_wait=no_wait,
        until_ready=until_ready,
        max_iterations=max_iterations,
        coin_ids=coin_ids,
        denomination_target=denomination_target,
        readiness_asset_id=str(setup.market.base_asset),
        run_step=_build_split_run_step(
            step_params=step_params,
            prompt_for_override=prompt_for_override,
        ),
    )
    final_readiness = loop_result.final_readiness or loop_result.split_gate
    success_payload = {
        **coin_op_result_payload(
            market=setup.market,
            selected_venue=setup.selected_venue,
            wallet=setup.wallet,
            coin_ids=coin_ids,
            denomination_target=denomination_target,
            until_ready=until_ready,
            max_iterations=max_iterations,
            stop_reason=loop_result.stop_reason,
            final_readiness=final_readiness,
            operations=loop_result.operations,
            fee_mojos=setup.fee_mojos,
            fee_source=setup.fee_source,
        ),
        "amount_per_coin": resolved_amount,
        "number_of_coins": resolved_count,
        "resolved_asset_id": setup.resolved_asset_id,
        "split_gate": loop_result.split_gate,
    }
    return _finish_coin_op_cli(
        setup=setup,
        loop_result=loop_result,
        until_ready=until_ready,
        success_payload=success_payload,
    )


def execute_combine_cli(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    network: str,
    market_id: str | None,
    pair: str | None,
    venue: str | None,
    coin_ids: list[str],
    number_of_coins: int,
    asset_id: str | None,
    no_wait: bool,
    size_base_units: int | None,
    until_ready: bool,
    max_iterations: int,
) -> int:
    requested_asset_id = asset_id.strip() if asset_id else None
    setup_result = coin_op_setup(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        network=network,
        market_id=market_id,
        pair=pair,
        venue=venue,
        canonical_asset_id_override=requested_asset_id,
    )
    if setup_result.error_payload is not None:
        print(format_json_output(setup_result.error_payload))
        return 2
    setup = setup_result.setup
    assert setup is not None

    resolved_count, denomination_target, combine_canonical_asset_id = _resolve_combine_targets(
        market=setup.market,
        number_of_coins=number_of_coins,
        size_base_units=size_base_units,
        requested_asset_id=requested_asset_id,
    )
    step_params = CoinCombineStepParams(
        wallet=setup.wallet,
        market=setup.market,
        selected_venue=setup.selected_venue,
        resolved_asset_id=setup.resolved_asset_id,
        combine_canonical_asset_id=combine_canonical_asset_id,
        explicit_coin_ids=coin_ids,
        number_of_coins=resolved_count,
        fee_mojos=setup.fee_mojos,
        denomination_target=denomination_target,
        min_coin_amount_mojos=coin_op_min_amount_mojos(
            canonical_asset_id=combine_canonical_asset_id
        ),
    )

    def run_step(
        iteration: int,
        wallet_coins: list[dict],
        existing_coin_ids: set[str],
    ) -> CoinOpStepOutcome:
        _ = iteration, existing_coin_ids
        step_result = run_coin_combine_step(params=step_params, wallet_coins=wallet_coins)
        return CoinOpStepOutcome(step=step_result.step)

    loop_result = run_coin_op_iteration_loop(
        wallet=setup.wallet,
        network=network,
        no_wait=no_wait,
        until_ready=until_ready,
        max_iterations=max_iterations,
        coin_ids=coin_ids,
        denomination_target=denomination_target,
        readiness_asset_id=setup.resolved_asset_id,
        run_step=run_step,
    )
    success_payload = {
        **coin_op_result_payload(
            market=setup.market,
            selected_venue=setup.selected_venue,
            wallet=setup.wallet,
            coin_ids=coin_ids,
            denomination_target=denomination_target,
            until_ready=until_ready,
            max_iterations=max_iterations,
            stop_reason=loop_result.stop_reason,
            final_readiness=loop_result.final_readiness,
            operations=loop_result.operations,
            fee_mojos=setup.fee_mojos,
            fee_source=setup.fee_source,
        ),
        "asset_id": requested_asset_id or str(setup.market.base_asset).strip(),
        "resolved_asset_id": setup.resolved_asset_id,
        "number_of_coins": resolved_count,
    }
    return _finish_coin_op_cli(
        setup=setup,
        loop_result=loop_result,
        until_ready=until_ready,
        success_payload=success_payload,
    )

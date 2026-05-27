"""Shared CLI orchestration for coin-split and coin-combine commands."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
from typing import Any

from greenfloor.runtime.cloud_wallet.adapter import format_json_output
from greenfloor.runtime.cloud_wallet.coin_op_errors import coin_op_unresolved_error_payload
from greenfloor.runtime.cloud_wallet.coin_ops_runtime import (
    CoinOpIterationStep,
    CoinOpLoopResult,
    CoinOpSetup,
    coin_op_result_payload,
    coin_op_setup,
    run_coin_op_iteration_loop,
)


def execute_coin_op_cli(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    network: str,
    market_id: str | None,
    pair: str | None,
    venue: str | None,
    coin_ids: list[str],
    no_wait: bool,
    until_ready: bool,
    max_iterations: int,
    denomination_target_for: Callable[[CoinOpSetup], dict[str, Any] | None],
    canonical_asset_id_override: str | None = None,
    readiness_asset_id_for: Callable[[CoinOpSetup], str],
    run_step_for: Callable[[CoinOpSetup], Callable[[int, list[dict], set[str]], CoinOpIterationStep]],
    build_success_payload: Callable[
        [CoinOpSetup, CoinOpLoopResult, dict[str, int | bool | str] | None],
        dict[str, object],
    ],
    resolve_final_readiness: Callable[
        [CoinOpLoopResult, dict[str, int | bool | str] | None],
        dict[str, int | bool | str] | None,
    ]
    | None = None,
) -> int:
    setup_result = coin_op_setup(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        network=network,
        market_id=market_id,
        pair=pair,
        venue=venue,
        canonical_asset_id_override=canonical_asset_id_override,
    )
    if setup_result.error_payload is not None:
        print(format_json_output(setup_result.error_payload))
        return 2
    setup = setup_result.setup
    assert setup is not None

    denomination_target = denomination_target_for(setup)
    loop_result = run_coin_op_iteration_loop(
        wallet=setup.wallet,
        network=network,
        no_wait=no_wait,
        until_ready=until_ready,
        max_iterations=max_iterations,
        coin_ids=coin_ids,
        denomination_target=denomination_target,
        readiness_asset_id=readiness_asset_id_for(setup),
        run_step=run_step_for(setup),
    )
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

    final_readiness = (
        resolve_final_readiness(loop_result, loop_result.final_readiness)
        if resolve_final_readiness is not None
        else loop_result.final_readiness
    )
    print(
        format_json_output(
            build_success_payload(setup, loop_result, final_readiness),
        )
    )
    if until_ready and final_readiness is not None and not bool(final_readiness["ready"]):
        return 2
    return 0


def build_split_run_step(
    *,
    step_params: Any,
    prompt_for_override: bool | None,
    split_gate_holder: dict[str, dict[str, int | bool | str] | None],
) -> Callable[[int, list[dict], set[str]], CoinOpIterationStep]:
    from greenfloor.cli.prompts import prompt_yes_no
    from greenfloor.runtime.cloud_wallet.coin_ops_runtime import CoinOpIterationNeedsConfirmation
    from greenfloor.runtime.cloud_wallet.coin_ops_steps import run_coin_split_step

    def run_step(
        iteration: int,
        wallet_coins: list[dict],
        existing_coin_ids: set[str],
    ) -> CoinOpIterationStep:
        _ = iteration, existing_coin_ids
        while True:
            step_result = run_coin_split_step(params=step_params, wallet_coins=wallet_coins)
            split_gate_holder["gate"] = step_result.split_gate
            step = step_result.step
            if isinstance(step, CoinOpIterationNeedsConfirmation):
                if prompt_yes_no(step.message, prompt_for_override=prompt_for_override):
                    if step.override == "force_split_when_ready":
                        step_params.force_split_when_ready = True
                    elif step.override == "allow_lock_all_spendable":
                        step_params.allow_lock_all_spendable = True
                    continue
                return step.decline_step
            return step

    return run_step

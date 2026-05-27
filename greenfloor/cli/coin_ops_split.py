"""CLI coin-split command."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from greenfloor.core.coin_ops_policy import coin_op_min_amount_mojos
from greenfloor.runtime.cloud_wallet.coin_ops_cli import build_split_run_step, execute_coin_op_cli
from greenfloor.runtime.cloud_wallet.coin_ops_runtime import (
    CoinOpLoopResult,
    CoinOpSetup,
    coin_op_result_payload,
    resolve_market_denomination_entry,
)
from greenfloor.runtime.cloud_wallet.coin_ops_steps import CoinSplitStepParams


@dataclass
class _SplitRunState:
    amount_per_coin: int
    number_of_coins: int
    denomination_target: dict[str, Any] | None = None
    split_gate: dict[str, int | bool | str] | None = None
    step_params: CoinSplitStepParams | None = field(default=None, repr=False)


def coin_split(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    network: str,
    market_id: str | None,
    pair: str | None,
    coin_ids: list[str],
    amount_per_coin: int,
    number_of_coins: int,
    no_wait: bool,
    venue: str | None = None,
    size_base_units: int | None = None,
    until_ready: bool = False,
    max_iterations: int = 3,
    allow_lock_all_spendable: bool = False,
    force_split_when_ready: bool = False,
    prompt_for_override: bool | None = None,
) -> int:
    if until_ready and no_wait:
        raise ValueError("until-ready mode requires wait mode (do not pass --no-wait)")
    if until_ready and size_base_units is None:
        raise ValueError("until-ready mode requires --size-base-units")
    if max_iterations <= 0:
        raise ValueError("max_iterations must be positive")

    run_state = _SplitRunState(amount_per_coin=amount_per_coin, number_of_coins=number_of_coins)
    split_gate_holder: dict[str, dict[str, int | bool | str] | None] = {"gate": None}

    def denomination_target_for(setup: CoinOpSetup) -> dict[str, Any] | None:
        market = setup.market
        resolved_amount = run_state.amount_per_coin
        resolved_count = run_state.number_of_coins
        denomination_target = None
        if size_base_units is not None and int(size_base_units) > 0:
            entry = resolve_market_denomination_entry(market, size_base_units=int(size_base_units))
            required_count = int(entry.target_count) + int(entry.split_buffer_count)
            if resolved_amount <= 0:
                resolved_amount = int(entry.size_base_units)
            elif resolved_amount != int(entry.size_base_units):
                raise ValueError(
                    "amount_per_coin must match market ladder size when --size-base-units is set"
                )
            if resolved_count <= 0:
                resolved_count = required_count
            elif resolved_count != required_count:
                raise ValueError(
                    "number_of_coins must match market ladder target+buffer when --size-base-units is set"
                )
            denomination_target = {
                "size_base_units": int(entry.size_base_units),
                "target_count": int(entry.target_count),
                "split_buffer_count": int(entry.split_buffer_count),
                "required_count": required_count,
            }
        elif resolved_amount <= 0:
            raise ValueError("amount_per_coin must be positive")
        elif resolved_count <= 0:
            raise ValueError("number_of_coins must be positive")
        run_state.amount_per_coin = resolved_amount
        run_state.number_of_coins = resolved_count
        run_state.denomination_target = denomination_target
        return denomination_target

    def run_step_for(setup: CoinOpSetup):
        market = setup.market
        step_params = CoinSplitStepParams(
            wallet=setup.wallet,
            market=market,
            selected_venue=setup.selected_venue,
            resolved_asset_id=setup.resolved_asset_id,
            explicit_coin_ids=coin_ids,
            amount_per_coin=run_state.amount_per_coin,
            number_of_coins=run_state.number_of_coins,
            fee_mojos=setup.fee_mojos,
            denomination_target=run_state.denomination_target,
            min_coin_amount_mojos=coin_op_min_amount_mojos(canonical_asset_id=str(market.base_asset)),
            allow_lock_all_spendable=allow_lock_all_spendable,
            force_split_when_ready=force_split_when_ready,
        )
        run_state.step_params = step_params
        return build_split_run_step(
            step_params=step_params,
            prompt_for_override=prompt_for_override,
            split_gate_holder=split_gate_holder,
        )

    def build_success_payload(
        setup: CoinOpSetup,
        loop_result: CoinOpLoopResult,
        final_readiness: dict[str, int | bool | str] | None,
    ) -> dict[str, object]:
        return {
            **coin_op_result_payload(
                market=setup.market,
                selected_venue=setup.selected_venue,
                wallet=setup.wallet,
                coin_ids=coin_ids,
                denomination_target=run_state.denomination_target,
                until_ready=until_ready,
                max_iterations=max_iterations,
                stop_reason=loop_result.stop_reason,
                final_readiness=final_readiness,
                operations=loop_result.operations,
                fee_mojos=setup.fee_mojos,
                fee_source=setup.fee_source,
            ),
            "amount_per_coin": run_state.amount_per_coin,
            "number_of_coins": run_state.number_of_coins,
            "resolved_asset_id": setup.resolved_asset_id,
            "split_gate": split_gate_holder["gate"],
        }

    def resolve_final_readiness(
        loop_result: CoinOpLoopResult,
        loop_final: dict[str, int | bool | str] | None,
    ) -> dict[str, int | bool | str] | None:
        _ = loop_result
        if loop_final is not None:
            return loop_final
        return split_gate_holder["gate"]

    return execute_coin_op_cli(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        network=network,
        market_id=market_id,
        pair=pair,
        venue=venue,
        coin_ids=coin_ids,
        no_wait=no_wait,
        until_ready=until_ready,
        max_iterations=max_iterations,
        denomination_target_for=denomination_target_for,
        readiness_asset_id_for=lambda setup: str(setup.market.base_asset),
        run_step_for=run_step_for,
        build_success_payload=build_success_payload,
        resolve_final_readiness=resolve_final_readiness,
    )

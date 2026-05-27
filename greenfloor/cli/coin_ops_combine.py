"""CLI coin-combine command."""

from __future__ import annotations

import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from greenfloor.core.coin_ops_policy import coin_op_min_amount_mojos
from greenfloor.runtime.cloud_wallet.coin_ops_cli import execute_coin_op_cli
from greenfloor.runtime.cloud_wallet.coin_ops_runtime import (
    CoinOpLoopResult,
    CoinOpSetup,
    coin_op_result_payload,
    resolve_market_denomination_entry,
)
from greenfloor.runtime.cloud_wallet.coin_ops_steps import CoinCombineStepParams, run_coin_combine_step


@dataclass
class _CombineRunState:
    number_of_coins: int
    denomination_target: dict[str, Any] | None = None
    combine_canonical_asset_id: str | None = None


def coin_combine(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    network: str,
    market_id: str | None,
    pair: str | None,
    number_of_coins: int,
    asset_id: str | None,
    coin_ids: list[str],
    no_wait: bool,
    venue: str | None = None,
    size_base_units: int | None = None,
    until_ready: bool = False,
    max_iterations: int = 3,
) -> int:
    if until_ready and no_wait:
        raise ValueError("until-ready mode requires wait mode (do not pass --no-wait)")
    if until_ready and size_base_units is None:
        raise ValueError("until-ready mode requires --size-base-units")
    if max_iterations <= 0:
        raise ValueError("max_iterations must be positive")

    requested_asset_id = asset_id.strip() if asset_id else None
    run_state = _CombineRunState(number_of_coins=number_of_coins)

    def denomination_target_for(setup: CoinOpSetup) -> dict[str, Any] | None:
        market = setup.market
        resolved_count = run_state.number_of_coins
        denomination_target = None
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
            denomination_target = {
                "size_base_units": int(entry.size_base_units),
                "target_count": int(entry.target_count),
                "combine_when_excess_factor": float(entry.combine_when_excess_factor),
                "combine_threshold_count": threshold,
            }
        if resolved_count <= 1:
            raise ValueError("number_of_coins must be > 1")
        run_state.number_of_coins = resolved_count
        run_state.denomination_target = denomination_target
        run_state.combine_canonical_asset_id = requested_asset_id or str(market.base_asset)
        return denomination_target

    def run_step_for(setup: CoinOpSetup):
        market = setup.market
        combine_canonical_asset_id = run_state.combine_canonical_asset_id or str(market.base_asset)
        step_params = CoinCombineStepParams(
            wallet=setup.wallet,
            market=market,
            selected_venue=setup.selected_venue,
            resolved_asset_id=setup.resolved_asset_id,
            combine_canonical_asset_id=combine_canonical_asset_id,
            explicit_coin_ids=coin_ids,
            number_of_coins=run_state.number_of_coins,
            fee_mojos=setup.fee_mojos,
            denomination_target=run_state.denomination_target,
            min_coin_amount_mojos=coin_op_min_amount_mojos(
                canonical_asset_id=combine_canonical_asset_id
            ),
        )

        def run_step(
            iteration: int,
            wallet_coins: list[dict],
            existing_coin_ids: set[str],
        ):
            _ = iteration, existing_coin_ids
            return run_coin_combine_step(params=step_params, wallet_coins=wallet_coins)

        return run_step

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
            "asset_id": requested_asset_id or str(setup.market.base_asset).strip(),
            "resolved_asset_id": setup.resolved_asset_id,
            "number_of_coins": run_state.number_of_coins,
        }

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
        canonical_asset_id_override=requested_asset_id,
        readiness_asset_id_for=lambda setup: setup.resolved_asset_id,
        run_step_for=run_step_for,
        build_success_payload=build_success_payload,
    )

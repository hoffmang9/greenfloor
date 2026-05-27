"""Signer coin-op CLI setup and iteration loop (coinset + Rust mixed-split)."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from greenfloor.config.io import (
    load_markets_config_with_optional_overlay,
    load_program_config,
    resolve_market_for_build,
)
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.runtime.cloud_wallet.coin_ops_runtime import (
    CoinOpIterationEarlyExit,
    CoinOpIterationExecuteResult,
    CoinOpIterationNeedsConfirmation,
    CoinOpIterationSkipLoop,
    CoinOpLoopResult,
    CoinOpStepOutcome,
    DenominationTarget,
    as_wait_events,
    coin_op_should_stop,
    resolve_venue_for_coin_prep,
)
from greenfloor.runtime.cloud_wallet.coins import is_spendable_coin
from greenfloor.runtime.signer_coin_ops import (
    list_signer_asset_coins,
    resolve_signer_asset_id,
)
from greenfloor.runtime.signer_coin_ops_steps import list_asset_scoped_coins_for_signer_step


@dataclass(slots=True)
class SignerCoinOpSetup:
    program: ProgramConfig
    market: MarketConfig
    receive_address: str
    resolved_asset_id: str
    fee_mojos: int
    fee_source: str
    selected_venue: str | None


@dataclass(slots=True)
class SignerCoinOpSetupResult:
    setup: SignerCoinOpSetup | None = None
    error_payload: dict[str, object] | None = None


def evaluate_signer_denomination_readiness(
    *,
    program: ProgramConfig,
    receive_address: str,
    asset_id: str,
    size_base_units: int,
    required_min_count: int | None = None,
    max_allowed_count: int | None = None,
) -> dict[str, int | bool | str]:
    coins = list_signer_asset_coins(
        program=program,
        receive_address=receive_address,
        asset_id=asset_id,
    )
    spendable = [
        c
        for c in coins
        if is_spendable_coin(c)
        and int(c.get("amount", 0)) == int(size_base_units)
    ]
    current_count = len(spendable)
    ready = True
    if required_min_count is not None:
        ready = current_count >= int(required_min_count)
    if max_allowed_count is not None:
        ready = ready and current_count <= int(max_allowed_count)
    return {
        "asset_id": asset_id,
        "size_base_units": int(size_base_units),
        "current_count": current_count,
        "required_min_count": int(required_min_count) if required_min_count is not None else -1,
        "max_allowed_count": int(max_allowed_count) if max_allowed_count is not None else -1,
        "ready": ready,
    }


def signer_coin_op_setup(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    network: str,
    market_id: str | None,
    pair: str | None,
    venue: str | None,
    canonical_asset_id_override: str | None = None,
) -> SignerCoinOpSetupResult:
    program = load_program_config(program_path)
    selected_venue = resolve_venue_for_coin_prep(venue_override=venue)
    markets = load_markets_config_with_optional_overlay(
        path=markets_path,
        overlay_path=testnet_markets_path,
    )
    market = resolve_market_for_build(
        markets,
        market_id=market_id,
        pair=pair,
        network=network,
    )
    receive_address = str(market.receive_address).strip()
    if not receive_address:
        return SignerCoinOpSetupResult(
            error_payload={
                "error": "signer_coin_ops_missing_receive_address",
                "market_id": market.market_id,
            }
        )
    canonical = canonical_asset_id_override or str(market.base_asset)
    resolved_asset_id = resolve_signer_asset_id(
        program,
        canonical_asset_id=canonical,
        symbol_hint=canonical_asset_id_override or str(market.base_symbol),
    )

    _ = network
    return SignerCoinOpSetupResult(
        setup=SignerCoinOpSetup(
            program=program,
            market=market,
            receive_address=receive_address,
            resolved_asset_id=resolved_asset_id,
            fee_mojos=0,
            fee_source="signer_vault_no_fee",
            selected_venue=selected_venue,
        )
    )


def signer_coin_op_result_payload(
    *,
    setup: SignerCoinOpSetup,
    coin_ids: list[str],
    denomination_target: DenominationTarget,
    until_ready: bool,
    max_iterations: int,
    stop_reason: str,
    final_readiness: dict[str, int | bool | str] | None,
    operations: list[dict[str, object]],
) -> dict[str, object]:
    return {
        "execution_backend": "signer",
        "market_id": setup.market.market_id,
        "pair": setup.market.pair,
        "selected_venue": setup.selected_venue,
        "resolved_asset_id": setup.resolved_asset_id,
        "receive_address": setup.receive_address,
        "coin_selection_mode": "explicit" if coin_ids else "signer_auto_select",
        "denomination_target": {
            "size_base_units": int(getattr(denomination_target, "size_base_units", 0)),
        },
        "until_ready": until_ready,
        "max_iterations": max_iterations,
        "stop_reason": stop_reason,
        "denomination_readiness": final_readiness,
        "operations": operations,
        "signature_request_id": (
            str(operations[-1].get("signature_request_id", "")) if operations else ""
        ),
        "signature_state": (
            str(operations[-1].get("signature_state", "UNKNOWN")) if operations else "UNKNOWN"
        ),
        "waited": bool(operations[-1].get("waited", False)) if operations else False,
        "wait_events": (
            as_wait_events(operations[-1].get("wait_events", [])) if operations else []
        ),
        "fee_mojos": setup.fee_mojos,
        "fee_source": setup.fee_source,
    }


def run_signer_coin_op_iteration_loop(
    *,
    setup: SignerCoinOpSetup,
    no_wait: bool,
    until_ready: bool,
    max_iterations: int,
    coin_ids: list[str],
    denomination_target: DenominationTarget,
    readiness_asset_id: str,
    run_step: Callable[[int, list[dict[str, Any]], set[str]], CoinOpStepOutcome],
) -> CoinOpLoopResult:
    operations: list[dict[str, object]] = []
    final_readiness: dict[str, int | bool | str] | None = None
    split_gate: dict[str, int | bool | str] | None = None
    stop_reason = "single_pass"
    unresolved_coin_ids: list[str] = []

    for iteration in range(1, max_iterations + 1):
        asset_coins = list_asset_scoped_coins_for_signer_step(
            program=setup.program,
            receive_address=setup.receive_address,
            resolved_asset_id=setup.resolved_asset_id,
        )
        existing_coin_ids = {
            str(c.get("id", c.get("name", ""))).strip() for c in asset_coins if c.get("id") or c.get("name")
        }
        outcome = run_step(iteration, asset_coins, existing_coin_ids)
        if outcome.split_gate is not None:
            split_gate = outcome.split_gate
        step = outcome.step

        if isinstance(step, CoinOpIterationEarlyExit):
            return CoinOpLoopResult(
                operations=operations,
                final_readiness=final_readiness,
                stop_reason=step.stop_reason or stop_reason,
                unresolved_coin_ids=list(step.unresolved_coin_ids or []),
                split_gate=split_gate,
                early_return_code=step.return_code,
                error_payload=step.error_payload,
            )
        if isinstance(step, CoinOpIterationSkipLoop):
            stop_reason = step.stop_reason
            break
        if isinstance(step, CoinOpIterationNeedsConfirmation):
            raise RuntimeError(
                "signer_coin_op_iteration_needs_confirmation must be handled before the loop"
            )

        assert isinstance(step, CoinOpIterationExecuteResult)
        iteration_payload: dict[str, object] = {
            "iteration": iteration,
            "signature_request_id": step.signature_request_id,
            "signature_state": step.initial_signature_state,
            "waited": not no_wait,
            "wait_events": [],
        }
        if denomination_target is not None:
            final_readiness = evaluate_signer_denomination_readiness(
                program=setup.program,
                receive_address=setup.receive_address,
                asset_id=readiness_asset_id,
                size_base_units=denomination_target.size_base_units,
                **step.readiness_kwargs,
            )
            iteration_payload["denomination_readiness"] = final_readiness
        operations.append(iteration_payload)

        should_break, reason = coin_op_should_stop(
            until_ready=until_ready,
            final_readiness=final_readiness,
            coin_ids=coin_ids,
            iteration=iteration,
            max_iterations=max_iterations,
        )
        if should_break:
            stop_reason = reason
            break

    return CoinOpLoopResult(
        operations=operations,
        final_readiness=final_readiness,
        stop_reason=stop_reason,
        unresolved_coin_ids=unresolved_coin_ids,
        split_gate=split_gate,
    )

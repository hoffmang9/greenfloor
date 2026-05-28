"""Shared signer coin-operation runtime (CLI, daemon, tests)."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal

from greenfloor.config.io import (
    load_markets_config_with_optional_overlay,
    load_program_config,
    resolve_market_for_build,
)
from greenfloor.config.models import MarketConfig, MarketLadderEntry, ProgramConfig
from greenfloor.core.coin_ops import coin_op_should_stop
from greenfloor.core.coin_ops.types import DenominationReadiness
from greenfloor.runtime.coin_ops.models import DenominationTarget, denomination_target_payload
from greenfloor.runtime.coin_ops.readiness import (
    build_coin_op_iteration_payload,
    evaluate_readiness_for_denomination_target,
)
from greenfloor.runtime.coin_ops_backend import (
    CoinOpBackend,
    SignerCoinOpBackend,
    build_coin_op_backend,
    resolve_coin_op_base_asset_id,
    resolve_signer_asset_id,
    scope_payload,
)


def as_wait_events(value: object) -> list[dict[str, str]]:
    if not isinstance(value, list):
        return []
    items: list[dict[str, str]] = []
    for row in value:
        if isinstance(row, dict):
            items.append({str(k): str(v) for k, v in row.items()})
    return items


def coin_op_result_payload(
    *,
    setup: CoinOpSetup,
    coin_ids: list[str],
    denomination_target: DenominationTarget,
    until_ready: bool,
    max_iterations: int,
    stop_reason: str,
    denomination_readiness: DenominationReadiness | None,
    operations: list[dict[str, object]],
) -> dict[str, object]:
    return {
        **scope_payload(setup.backend.scope),
        "coin_selection_mode": "explicit" if coin_ids else "adapter_auto_select",
        "denomination_target": denomination_target_payload(denomination_target),
        "until_ready": until_ready,
        "max_iterations": max_iterations,
        "stop_reason": stop_reason,
        "denomination_readiness": (
            None if denomination_readiness is None else denomination_readiness.to_payload()
        ),
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


def resolve_venue_for_coin_prep(*, venue_override: str | None) -> str | None:
    if venue_override is None or not venue_override.strip():
        return None
    venue = venue_override.strip().lower()
    if venue not in {"dexie", "splash"}:
        raise ValueError("coin-prep venue must be dexie or splash when provided")
    return venue


def resolve_market_denomination_entry(
    market: MarketConfig, *, size_base_units: int
) -> MarketLadderEntry:
    ladder = market.ladders.get("sell") or []
    if not ladder:
        raise ValueError(
            f"market {market.market_id} has no sell ladder; cannot resolve denomination target"
        )
    for entry in ladder:
        if int(entry.size_base_units) == int(size_base_units):
            return entry
    allowed = ", ".join(str(int(row.size_base_units)) for row in ladder)
    raise ValueError(
        f"size_base_units not configured for market sell ladder; use one of: {allowed}"
    )


@dataclass(slots=True)
class CoinOpSetup:
    program: ProgramConfig
    market: MarketConfig
    backend: CoinOpBackend
    resolved_asset_id: str
    fee_mojos: int
    fee_source: str
    selected_venue: str | None


@dataclass(slots=True)
class CoinOpSetupResult:
    setup: CoinOpSetup | None = None
    error_payload: dict[str, object] | None = None


def coin_op_setup(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    network: str,
    market_id: str | None,
    pair: str | None,
    venue: str | None,
    canonical_asset_id_override: str | None = None,
) -> CoinOpSetupResult:
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
    canonical = canonical_asset_id_override or str(market.base_asset)
    hint = canonical_asset_id_override or str(market.base_symbol)
    try:
        if canonical_asset_id_override:
            resolved_asset_id = resolve_signer_asset_id(
                program, canonical_asset_id=canonical, symbol_hint=hint
            )
        else:
            resolved_asset_id = resolve_coin_op_base_asset_id(program=program, market=market)
        backend = build_coin_op_backend(
            program=program,
            market=market,
            selected_venue=selected_venue,
            resolved_asset_id=resolved_asset_id,
        )
    except ValueError as exc:
        return CoinOpSetupResult(
            error_payload={
                "error": str(exc),
                "market_id": market.market_id,
            }
        )
    return CoinOpSetupResult(
        setup=CoinOpSetup(
            program=program,
            market=market,
            backend=backend,
            resolved_asset_id=resolved_asset_id,
            fee_mojos=0,
            fee_source="signer_vault_no_fee",
            selected_venue=selected_venue,
        )
    )


@dataclass(slots=True)
class CoinOpIterationExecuteResult:
    signature_request_id: str
    initial_signature_state: str
    refresh_readiness_after_execute: bool = True


@dataclass(slots=True)
class CoinOpIterationEarlyExit:
    return_code: int
    unresolved_coin_ids: list[str] | None = None
    stop_reason: str = ""
    error_payload: dict[str, object] | None = None


@dataclass(slots=True)
class CoinOpIterationSkipLoop:
    stop_reason: str


@dataclass(slots=True)
class CoinOpIterationNeedsConfirmation:
    message: str
    override: Literal["force_split_when_ready", "allow_lock_all_spendable"]
    decline_step: CoinOpIterationEarlyExit | CoinOpIterationSkipLoop


CoinOpIterationStep = (
    CoinOpIterationExecuteResult
    | CoinOpIterationEarlyExit
    | CoinOpIterationSkipLoop
    | CoinOpIterationNeedsConfirmation
)


@dataclass(slots=True)
class CoinOpStepOutcome:
    step: CoinOpIterationStep
    denomination_readiness: DenominationReadiness | None = None


@dataclass(slots=True)
class CoinOpLoopResult:
    operations: list[dict[str, object]]
    denomination_readiness: DenominationReadiness | None
    stop_reason: str
    unresolved_coin_ids: list[str]
    early_return_code: int | None = None
    error_payload: dict[str, object] | None = None


def run_coin_op_iteration_loop(
    *,
    setup: CoinOpSetup,
    network: str,
    no_wait: bool,
    until_ready: bool,
    max_iterations: int,
    coin_ids: list[str],
    denomination_target: DenominationTarget,
    readiness_asset_id: str,
    run_step: Callable[
        [int, list[dict[str, Any]], set[str], DenominationReadiness | None],
        CoinOpStepOutcome,
    ],
) -> CoinOpLoopResult:
    _ = network
    backend = setup.backend
    if isinstance(backend, SignerCoinOpBackend):
        backend.no_wait = no_wait
    operations: list[dict[str, object]] = []
    denomination_readiness: DenominationReadiness | None = None
    stop_reason = "single_pass"
    unresolved_coin_ids: list[str] = []

    for iteration in range(1, max_iterations + 1):
        wallet_coins = backend.list_wallet_coins()
        existing_coin_ids = {
            str(c.get("id", c.get("name", ""))).strip()
            for c in wallet_coins
            if str(c.get("id", c.get("name", ""))).strip()
        }
        pre_readiness = evaluate_readiness_for_denomination_target(
            asset_scoped_coins=backend.list_asset_scoped_coins(),
            asset_id=readiness_asset_id,
            target=denomination_target,
        )
        outcome = run_step(iteration, wallet_coins, existing_coin_ids, pre_readiness)
        if outcome.denomination_readiness is not None:
            denomination_readiness = outcome.denomination_readiness
        elif pre_readiness is not None:
            denomination_readiness = pre_readiness
        step = outcome.step

        if isinstance(step, CoinOpIterationEarlyExit):
            return CoinOpLoopResult(
                operations=operations,
                denomination_readiness=denomination_readiness,
                stop_reason=step.stop_reason or stop_reason,
                unresolved_coin_ids=list(step.unresolved_coin_ids or []),
                early_return_code=step.return_code,
                error_payload=step.error_payload,
            )
        if isinstance(step, CoinOpIterationSkipLoop):
            stop_reason = step.stop_reason
            break
        if isinstance(step, CoinOpIterationNeedsConfirmation):
            raise RuntimeError(
                "coin_op_iteration_needs_confirmation must be handled before the loop"
            )

        iteration_payload, denomination_readiness = build_coin_op_iteration_payload(
            operation_id=step.signature_request_id,
            operation_state=step.initial_signature_state,
            no_wait=no_wait,
            iteration=iteration,
            readiness_asset_id=readiness_asset_id,
            denomination_target=denomination_target,
            asset_scoped_coins=backend.list_asset_scoped_coins(),
            readiness=pre_readiness,
            refresh_readiness=step.refresh_readiness_after_execute,
        )
        operations.append(iteration_payload)

        readiness_ready = None if denomination_readiness is None else denomination_readiness.ready
        should_break, reason = coin_op_should_stop(
            until_ready=until_ready,
            readiness_ready=readiness_ready,
            coin_ids=coin_ids,
            iteration=iteration,
            max_iterations=max_iterations,
        )
        if should_break:
            stop_reason = reason
            break

    return CoinOpLoopResult(
        operations=operations,
        denomination_readiness=denomination_readiness,
        stop_reason=stop_reason,
        unresolved_coin_ids=unresolved_coin_ids,
    )

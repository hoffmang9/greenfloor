"""Bootstrap ladder shaping and mixed-split execution for signer offer runtime.

Rust owns deterministic planner and phase status/reason mapping (via
``core.offer_bootstrap_bridge``). This module owns fee resolution, Coinset coin I/O,
vault mixed-split submission, confirmation wait, and post-split replan orchestration.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, replace
from typing import Any, Protocol

from greenfloor.adapters import rust_signer
from greenfloor.config.models import MarketLadderEntry, ProgramConfig
from greenfloor.core.offer_bootstrap_bridge import (
    BootstrapPhaseResult,
    BootstrapPlan,
    BootstrapPlanOutcome,
    PlannerLadderRow,
    bootstrap_early_phase,
    bootstrap_executed_phase,
)
from greenfloor.core.signer_offer_request import (
    quote_mojos_for_base_size,
    resolve_quote_unit_multiplier,
)


class ListBootstrapCoinsFn(Protocol):
    def __call__(
        self,
        *,
        network: str,
        receive_address: str,
        asset_id: str,
    ) -> list[dict[str, Any]]: ...


class WaitForBootstrapConfirmationFn(Protocol):
    def __call__(
        self,
        *,
        network: str,
        receive_address: str,
        asset_id: str,
        initial_coin_ids: set[str],
        timeout_seconds: int,
    ) -> list[dict[str, str]]: ...


class IsSpendableBootstrapCoinFn(Protocol):
    def __call__(self, coin: dict[str, Any], /) -> bool: ...


class PlanBootstrapMixedOutputsFn(Protocol):
    def __call__(
        self,
        *,
        ladder_entries: list[PlannerLadderRow],
        spendable_coins: list[Any],
    ) -> BootstrapPlanOutcome: ...


class ResolveBootstrapSplitFeeFn(Protocol):
    def __call__(
        self,
        *,
        network: str,
        minimum_fee_mojos: int,
        output_count: int,
    ) -> tuple[int, str, str | None]: ...


@dataclass(frozen=True)
class BootstrapRuntimeDeps:
    """Injectable runtime adapters for bootstrap coin I/O and replanning."""

    list_bootstrap_coins_fn: ListBootstrapCoinsFn
    wait_for_confirmation_fn: WaitForBootstrapConfirmationFn
    is_spendable_coin_fn: IsSpendableBootstrapCoinFn
    plan_bootstrap_mixed_outputs_fn: PlanBootstrapMixedOutputsFn


@dataclass(frozen=True)
class BootstrapPreflight:
    """Resolved bootstrap inputs ready for mixed-split execution."""

    program: ProgramConfig
    bootstrap_plan: BootstrapPlan
    split_asset_id: str
    receive_address: str
    fee_mojos: int
    fee_source: str
    fee_lookup_error: str | None
    existing_coin_ids: set[str]
    bootstrap_wait_timeout_seconds: int
    ladder_entries: list[PlannerLadderRow]
    deps: BootstrapRuntimeDeps


@dataclass(frozen=True)
class BootstrapSplitExecution:
    """Inputs for one vault mixed-split bootstrap submission."""

    preflight: BootstrapPreflight
    config_path: str


@dataclass(frozen=True)
class BootstrapPreflightOutcome:
    """Either a terminal early phase result or inputs ready for mixed-split."""

    early: BootstrapPhaseResult | None = None
    preflight: BootstrapPreflight | None = None

    def __post_init__(self) -> None:
        if (self.early is None) == (self.preflight is None):
            raise ValueError("exactly one of early or preflight must be set")

    @classmethod
    def early_exit(cls, result: BootstrapPhaseResult) -> BootstrapPreflightOutcome:
        return cls(early=result, preflight=None)

    @classmethod
    def ready(cls, preflight: BootstrapPreflight) -> BootstrapPreflightOutcome:
        return cls(early=None, preflight=preflight)


def bootstrap_ladder_entries_for_side(
    *,
    side: str,
    side_ladder: Sequence[MarketLadderEntry],
    pricing: dict[str, Any],
    quote_price: float,
    resolved_quote_asset_id: str,
) -> list[PlannerLadderRow]:
    """Normalize market ladder rows into planner inputs for sell or buy bootstrap."""
    quote_unit_multiplier: int | None = None
    if side == "buy":
        quote_unit_multiplier = resolve_quote_unit_multiplier(
            pricing=pricing,
            resolved_quote_asset_id=str(resolved_quote_asset_id),
        )

    entries: list[PlannerLadderRow] = []
    for entry in side_ladder:
        size_base_units = int(entry.size_base_units)
        if quote_unit_multiplier is not None:
            size_base_units = quote_mojos_for_base_size(
                size_base_units=size_base_units,
                quote_price=float(quote_price),
                quote_unit_multiplier=quote_unit_multiplier,
            )
            if size_base_units <= 0:
                continue
        entries.append(
            PlannerLadderRow(
                size_base_units=size_base_units,
                target_count=int(entry.target_count),
                split_buffer_count=int(entry.split_buffer_count),
            )
        )
    return entries


def run_bootstrap_preflight(
    *,
    program: ProgramConfig,
    ladder_entries: list[PlannerLadderRow],
    split_asset_id: str,
    receive_address: str,
    spendable_coins: list[dict[str, Any]],
    asset_scoped_coins: list[dict[str, Any]],
    bootstrap_wait_timeout_seconds: int,
    minimum_fee_mojos: int,
    deps: BootstrapRuntimeDeps,
    resolve_bootstrap_split_fee_fn: ResolveBootstrapSplitFeeFn,
) -> BootstrapPreflightOutcome:
    """Plan bootstrap, resolve fees, and return early exit or execution context."""
    outcome = deps.plan_bootstrap_mixed_outputs_fn(
        ladder_entries=ladder_entries,
        spendable_coins=spendable_coins,
    )
    early = bootstrap_early_phase(outcome=outcome)
    if early is not None:
        return BootstrapPreflightOutcome.early_exit(early)
    if outcome.plan is None:
        raise RuntimeError("bootstrap_failed:planner_missing_plan")

    bootstrap_plan = outcome.plan
    fee_mojos, fee_source, fee_lookup_error = resolve_bootstrap_split_fee_fn(
        network=str(program.app_network),
        minimum_fee_mojos=int(minimum_fee_mojos),
        output_count=len(bootstrap_plan.output_amounts_base_units),
    )
    if int(fee_mojos) > 0:
        return BootstrapPreflightOutcome.early_exit(
            BootstrapPhaseResult(
                status="failed",
                reason="signer_mixed_split_fee_not_supported",
                fee_mojos=int(fee_mojos),
                fee_source=fee_source,
                fee_lookup_error=fee_lookup_error,
            )
        )

    existing_coin_ids = {
        str(c.get("id", "")).strip() for c in asset_scoped_coins if str(c.get("id", "")).strip()
    }
    return BootstrapPreflightOutcome.ready(
        BootstrapPreflight(
            program=program,
            bootstrap_plan=bootstrap_plan,
            split_asset_id=split_asset_id,
            receive_address=receive_address,
            fee_mojos=int(fee_mojos),
            fee_source=fee_source,
            fee_lookup_error=fee_lookup_error,
            existing_coin_ids=existing_coin_ids,
            bootstrap_wait_timeout_seconds=bootstrap_wait_timeout_seconds,
            ladder_entries=ladder_entries,
            deps=deps,
        )
    )


def execute_bootstrap_mixed_split(execution: BootstrapSplitExecution) -> BootstrapPhaseResult:
    """Submit vault mixed-split from a planner result, wait, and report readiness."""
    preflight = execution.preflight
    deps = preflight.deps
    bootstrap_plan = preflight.bootstrap_plan
    output_amounts = [int(amount) for amount in bootstrap_plan.output_amounts_base_units]
    split_request = {
        "receive_address": preflight.receive_address,
        "asset_id": preflight.split_asset_id.removeprefix("0x"),
        "output_amounts": output_amounts,
        "coin_ids": [bootstrap_plan.source_coin_id.removeprefix("0x")],
        "allow_sub_cat_output": False,
        "fee_mojos": 0,
        "broadcast": True,
    }
    try:
        split_result = rust_signer.build_mixed_split(execution.config_path, split_request)
    except Exception as exc:
        return BootstrapPhaseResult(
            status="failed",
            reason=f"signer_mixed_split_error:{exc}",
            fee_mojos=int(preflight.fee_mojos),
            fee_source=preflight.fee_source,
            fee_lookup_error=preflight.fee_lookup_error,
        )

    wait_events: list[dict[str, str]] = []
    wait_error: str | None = None
    try:
        wait_events = deps.wait_for_confirmation_fn(
            network=str(preflight.program.app_network),
            receive_address=preflight.receive_address,
            asset_id=preflight.split_asset_id,
            initial_coin_ids=preflight.existing_coin_ids,
            timeout_seconds=max(10, int(preflight.bootstrap_wait_timeout_seconds)),
        )
    except Exception as exc:
        wait_error = str(exc)
        return BootstrapPhaseResult(
            status="failed",
            reason="bootstrap_wait_failed",
            wait_error=wait_error,
            fee_mojos=int(preflight.fee_mojos),
            fee_source=preflight.fee_source,
            fee_lookup_error=preflight.fee_lookup_error,
            split_result=dict(split_result) if isinstance(split_result, dict) else {},
            wait_events=wait_events,
        )

    refreshed_asset_coins = deps.list_bootstrap_coins_fn(
        network=str(preflight.program.app_network),
        receive_address=preflight.receive_address,
        asset_id=preflight.split_asset_id,
    )
    refreshed_spendable = [
        coin for coin in refreshed_asset_coins if deps.is_spendable_coin_fn(coin)
    ]
    remaining_outcome = deps.plan_bootstrap_mixed_outputs_fn(
        ladder_entries=preflight.ladder_entries,
        spendable_coins=refreshed_spendable,
    )
    executed = bootstrap_executed_phase(remaining=remaining_outcome)
    return replace(
        executed,
        fee_mojos=int(preflight.fee_mojos),
        fee_source=preflight.fee_source,
        fee_lookup_error=preflight.fee_lookup_error,
        wait_error=wait_error,
        split_result=dict(split_result) if isinstance(split_result, dict) else {},
        wait_events=wait_events,
        plan=bootstrap_plan,
    )

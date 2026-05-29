"""Bootstrap ladder shaping and mixed-split execution for signer offer runtime."""

from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass, field
from typing import Any, Literal

from greenfloor.adapters import rust_signer
from greenfloor.config.models import MarketLadderEntry, ProgramConfig
from greenfloor.core.offer_bootstrap_policy import (
    BootstrapPlan,
    BootstrapPlanOutcome,
    PlannerLadderRow,
)
from greenfloor.core.signer_offer_request import (
    quote_mojos_for_base_size,
    resolve_quote_unit_multiplier,
)

BootstrapPhaseStatus = Literal["skipped", "failed", "executed"]


@dataclass(frozen=True)
class BootstrapPhaseResult:
    """Typed bootstrap phase output for offer orchestration."""

    status: BootstrapPhaseStatus
    reason: str
    ready: bool = False
    fee_mojos: int = 0
    fee_source: str = ""
    fee_lookup_error: str | None = None
    wait_error: str | None = None
    split_result: dict[str, Any] = field(default_factory=dict)
    wait_events: list[dict[str, str]] = field(default_factory=list)
    plan: dict[str, Any] | None = None

    def to_manager_dict(self) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "status": self.status,
            "reason": self.reason,
            "ready": self.ready,
            "fee_mojos": self.fee_mojos,
            "fee_source": self.fee_source,
            "fee_lookup_error": self.fee_lookup_error,
        }
        if self.wait_error is not None:
            payload["wait_error"] = self.wait_error
        if self.split_result:
            payload["split_result"] = dict(self.split_result)
        if self.wait_events:
            payload["wait_events"] = list(self.wait_events)
        if self.plan is not None:
            payload["plan"] = dict(self.plan)
        return payload


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
    spendable_coins: list[dict[str, Any]]
    list_bootstrap_coins_fn: Callable[..., list[dict[str, Any]]]
    wait_for_confirmation_fn: Callable[..., list[dict[str, str]]]
    is_spendable_coin_fn: Callable[[dict], bool]
    plan_bootstrap_mixed_outputs_fn: Callable[..., BootstrapPlanOutcome]


@dataclass(frozen=True)
class BootstrapSplitExecution:
    """Inputs for one vault mixed-split bootstrap submission."""

    preflight: BootstrapPreflight
    config_path: str


def phase_result_for_planner_outcome(outcome: BootstrapPlanOutcome) -> BootstrapPhaseResult | None:
    """Map kernel planner outcome to an early bootstrap phase result, if any."""
    if outcome.kind == "ready":
        return BootstrapPhaseResult(status="skipped", reason="already_ready")
    if outcome.kind == "cannot_fund":
        total = int(outcome.total_output_amount or 0)
        return BootstrapPhaseResult(
            status="skipped",
            reason=f"bootstrap_underfunded:total_output_amount={total}",
        )
    if outcome.kind == "invalid_ladder":
        return BootstrapPhaseResult(
            status="failed",
            reason="bootstrap_failed:bootstrap_invalid_ladder",
        )
    if outcome.kind == "invalid_coins":
        return BootstrapPhaseResult(
            status="failed",
            reason="bootstrap_failed:bootstrap_invalid_coins",
        )
    if outcome.kind == "needs_split":
        return None
    raise ValueError(f"unsupported_bootstrap_plan_outcome:{outcome.kind}")


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
    list_bootstrap_coins_fn: Callable[..., list[dict[str, Any]]],
    wait_for_confirmation_fn: Callable[..., list[dict[str, str]]],
    is_spendable_coin_fn: Callable[[dict], bool],
    plan_bootstrap_mixed_outputs_fn: Callable[..., BootstrapPlanOutcome],
    resolve_bootstrap_split_fee_fn: Callable[..., tuple[int, str, str | None]],
) -> BootstrapPhaseResult | BootstrapPreflight:
    """Plan bootstrap, resolve fees, and return either an early result or execution context."""
    outcome = plan_bootstrap_mixed_outputs_fn(
        ladder_entries=ladder_entries,
        spendable_coins=spendable_coins,
    )
    early = phase_result_for_planner_outcome(outcome)
    if early is not None:
        return early
    if outcome.plan is None:
        return BootstrapPhaseResult(
            status="failed",
            reason="bootstrap_failed:planner_missing_plan",
        )

    fee_mojos, fee_source, fee_lookup_error = resolve_bootstrap_split_fee_fn(
        network=str(program.app_network),
        minimum_fee_mojos=int(minimum_fee_mojos),
        output_count=len(outcome.plan.output_amounts_base_units),
    )
    if int(fee_mojos) > 0:
        return BootstrapPhaseResult(
            status="failed",
            reason="bootstrap_failed:signer_mixed_split_fee_not_supported",
            fee_mojos=int(fee_mojos),
            fee_source=fee_source,
            fee_lookup_error=fee_lookup_error,
        )

    existing_coin_ids = {
        str(c.get("id", "")).strip() for c in asset_scoped_coins if str(c.get("id", "")).strip()
    }
    return BootstrapPreflight(
        program=program,
        bootstrap_plan=outcome.plan,
        split_asset_id=split_asset_id,
        receive_address=receive_address,
        fee_mojos=int(fee_mojos),
        fee_source=fee_source,
        fee_lookup_error=fee_lookup_error,
        existing_coin_ids=existing_coin_ids,
        bootstrap_wait_timeout_seconds=bootstrap_wait_timeout_seconds,
        ladder_entries=ladder_entries,
        spendable_coins=spendable_coins,
        list_bootstrap_coins_fn=list_bootstrap_coins_fn,
        wait_for_confirmation_fn=wait_for_confirmation_fn,
        is_spendable_coin_fn=is_spendable_coin_fn,
        plan_bootstrap_mixed_outputs_fn=plan_bootstrap_mixed_outputs_fn,
    )


def execute_bootstrap_mixed_split(execution: BootstrapSplitExecution) -> BootstrapPhaseResult:
    """Submit vault mixed-split from a planner result, wait, and report readiness."""
    preflight = execution.preflight
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
            reason=f"bootstrap_failed:signer_mixed_split_error:{exc}",
            fee_mojos=int(preflight.fee_mojos),
            fee_source=preflight.fee_source,
            fee_lookup_error=preflight.fee_lookup_error,
        )

    wait_events: list[dict[str, str]] = []
    wait_error: str | None = None
    try:
        wait_events = preflight.wait_for_confirmation_fn(
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

    refreshed_asset_coins = preflight.list_bootstrap_coins_fn(
        network=str(preflight.program.app_network),
        receive_address=preflight.receive_address,
        asset_id=preflight.split_asset_id,
    )
    refreshed_spendable = [
        coin for coin in refreshed_asset_coins if preflight.is_spendable_coin_fn(coin)
    ]
    remaining_outcome = preflight.plan_bootstrap_mixed_outputs_fn(
        ladder_entries=preflight.ladder_entries,
        spendable_coins=refreshed_spendable,
    )
    return BootstrapPhaseResult(
        status="executed",
        reason="bootstrap_submitted",
        ready=remaining_outcome.kind == "ready",
        fee_mojos=int(preflight.fee_mojos),
        fee_source=preflight.fee_source,
        fee_lookup_error=preflight.fee_lookup_error,
        wait_error=wait_error,
        split_result=dict(split_result) if isinstance(split_result, dict) else {},
        wait_events=wait_events,
        plan={
            "source_coin_id": bootstrap_plan.source_coin_id,
            "source_amount": bootstrap_plan.source_amount,
            "output_count": len(bootstrap_plan.output_amounts_base_units),
            "total_output_amount": bootstrap_plan.total_output_amount,
            "change_amount": bootstrap_plan.change_amount,
        },
    )

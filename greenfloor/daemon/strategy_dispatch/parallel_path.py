"""Parallel managed-offer dispatch with reservation batching."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.cycle import plan_parallel_managed_dispatch
from greenfloor.core.parallel_reservation_context import parallel_reservation_asset_ids
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.inventory_scan import coinset_spendable_profiles_by_asset
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.daemon.strategy_dispatch.items import managed_skip_item, strategy_action_result
from greenfloor.daemon.strategy_dispatch.parallel_pool import run_parallel_batch_submissions
from greenfloor.daemon.strategy_dispatch.reservation_helpers import (
    parallel_reservation_context,
    reservation_wallet_id,
)
from greenfloor.daemon.strategy_dispatch.runtime import StrategyDispatchHooks


def execute_actions_parallel(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    expanded_actions: list[PlannedAction],
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
    reservation_coordinator: AssetReservationCoordinator,
    hooks: StrategyDispatchHooks,
) -> dict[str, Any]:
    items: list[StrategyActionItem] = []
    executed_count = 0
    resolved_base_asset_id, resolved_quote_asset_id, resolved_xch_asset_id = (
        hooks.resolve_signer_offer_asset_ids_for_reservation(
            program=program,
            market=market,
        )
    )
    wallet_id = reservation_wallet_id(program)
    reservation_coordinator.probe_storage()

    reservation_ctx = parallel_reservation_context(
        market=market,
        resolved_base_asset_id=resolved_base_asset_id,
        resolved_quote_asset_id=resolved_quote_asset_id,
        resolved_xch_asset_id=resolved_xch_asset_id,
    )
    spendable_profiles = coinset_spendable_profiles_by_asset(
        program=program,
        market=market,
        asset_ids=parallel_reservation_asset_ids(reservation_ctx),
    )
    batch_plan = plan_parallel_managed_dispatch(
        actions=expanded_actions,
        ctx=reservation_ctx,
        spendable_profiles=spendable_profiles,
    )
    items.extend(
        managed_skip_item(
            action=expanded_actions[skip.submit_index],
            reason=skip.reason,
        )
        for skip in batch_plan.skip_items
    )

    if batch_plan.queue:
        queued_executed, queued_items = run_parallel_batch_submissions(
            program=program,
            market=market,
            expanded_actions=expanded_actions,
            batch_plan=batch_plan,
            publish_venue=publish_venue,
            runtime_dry_run=runtime_dry_run,
            dexie=dexie,
            reservation_coordinator=reservation_coordinator,
            wallet_id=wallet_id,
            hooks=hooks,
        )
        executed_count += queued_executed
        items.extend(queued_items)

    return strategy_action_result(
        planned_count=len(expanded_actions),
        executed_count=executed_count,
        items=items,
    )

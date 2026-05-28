"""Parallel managed-offer dispatch with reservation batching."""

from __future__ import annotations

import concurrent.futures
import time
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.cycle import (
    count_parallel_transient_failures,
    parallel_max_workers,
    plan_parallel_managed_dispatch,
    should_apply_parallel_transient_cooldown,
)
from greenfloor.core.cycle_orchestration import ParallelActionOutcome
from greenfloor.core.parallel_reservation_prep import (
    ParallelActionReservationInput,
    parallel_reservation_asset_ids,
)
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.cooldowns import _POST_COOLDOWN_UNTIL, _post_retry_config, _set_cooldown
from greenfloor.daemon.inventory_scan import coinset_spendable_profiles_by_asset
from greenfloor.daemon.market_helpers import _normalize_offer_side
from greenfloor.daemon.market_logging import _log_market_decision, _log_offer_action_timing
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.daemon.strategy_dispatch.items import (
    managed_skip_item,
    parallel_offer_worker_error_item,
    strategy_action_result,
)
from greenfloor.daemon.strategy_dispatch.parallel_worker import run_parallel_submission
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
    reservation_inputs = [
        ParallelActionReservationInput(
            submit_index=submit_index,
            side=_normalize_offer_side(action.side),
            size_base_units=int(action.size),
        )
        for submit_index, action in enumerate(expanded_actions)
    ]
    spendable_profiles = coinset_spendable_profiles_by_asset(
        program=program,
        market=market,
        asset_ids=parallel_reservation_asset_ids(reservation_ctx),
    )
    batch_plan = plan_parallel_managed_dispatch(
        actions=reservation_inputs,
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

    if not batch_plan.queue:
        return strategy_action_result(
            planned_count=len(expanded_actions),
            executed_count=executed_count,
            items=items,
        )

    max_workers = parallel_max_workers(
        submission_count=len(batch_plan.queue),
        configured_max=int(program.runtime_offer_parallelism_max_workers),
    )
    _log_market_decision(
        str(market.market_id),
        "parallel_offer_dispatch",
        planned_count=len(expanded_actions),
        queued_count=len(batch_plan.queue),
        workers=max_workers,
    )

    with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
        future_to_submission: dict[concurrent.futures.Future[StrategyActionItem], int] = {}
        for queue_item in batch_plan.queue:
            future = pool.submit(
                run_parallel_submission,
                queue_item=queue_item,
                action=expanded_actions[queue_item.submit_index],
                market=market,
                program=program,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
                reservation_coordinator=reservation_coordinator,
                wallet_id=wallet_id,
                hooks=hooks,
                queued_at_monotonic=time.monotonic(),
            )
            future_to_submission[future] = queue_item.submit_index
        submitted_items: list[tuple[int, StrategyActionItem]] = []
        for future in concurrent.futures.as_completed(future_to_submission):
            submit_index = future_to_submission[future]
            try:
                item = future.result()
            except Exception as exc:
                item = parallel_offer_worker_error_item(exc=exc)
            submitted_items.append((submit_index, item))
        for _, item in sorted(submitted_items, key=lambda pair: pair[0]):
            _log_offer_action_timing(str(market.market_id), item)
            if item.is_executed:
                executed_count += 1
            items.append(item)

    _, _, cooldown_seconds = _post_retry_config()
    transient_parallel_failures = count_parallel_transient_failures(
        [
            ParallelActionOutcome(
                status=item.status,
                transient_upstream=item.transient_upstream,
            )
            for _submit_idx, item in submitted_items
        ]
    )
    total_parallel = len(submitted_items)
    if should_apply_parallel_transient_cooldown(
        transient_failures=transient_parallel_failures,
        total_parallel=total_parallel,
        cooldown_seconds=int(cooldown_seconds),
    ):
        cooldown_key = f"{publish_venue}:{market.market_id}"
        _set_cooldown(_POST_COOLDOWN_UNTIL, cooldown_key, cooldown_seconds)
        _log_market_decision(
            str(market.market_id),
            "parallel_offer_transient_cooldown",
            transient_failures=transient_parallel_failures,
            total_parallel=total_parallel,
            cooldown_seconds=cooldown_seconds,
        )
    return strategy_action_result(
        planned_count=len(expanded_actions),
        executed_count=executed_count,
        items=items,
    )

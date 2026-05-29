"""Parallel managed-offer dispatch: planning, reservation workers, and pool execution."""

from __future__ import annotations

import concurrent.futures
import time

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.cycle import (
    count_parallel_transient_failures,
    parallel_max_workers,
    plan_parallel_managed_dispatch,
    reservation_release_status,
    should_apply_parallel_transient_cooldown,
)
from greenfloor.core.parallel_batch_plan import ParallelBatchPlan, ParallelQueueItem
from greenfloor.core.parallel_reservation_context import parallel_reservation_asset_ids
from greenfloor.core.planned_action import PlannedAction, planned_action_side
from greenfloor.core.strategy_action_item import StrategyActionItem
from greenfloor.daemon.cooldowns import _POST_COOLDOWN_UNTIL, _post_retry_config, _set_cooldown
from greenfloor.daemon.inventory_scan import coinset_spendable_profiles_by_asset
from greenfloor.daemon.market_logging import _log_market_decision, _log_offer_action_timing
from greenfloor.daemon.offer_dispatch.items import (
    managed_skip_item,
    parallel_offer_worker_error_item,
)
from greenfloor.daemon.offer_dispatch.reservation import (
    parallel_reservation_context,
    reservation_wallet_id,
)
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_execution import StrategyActionResult, StrategyDispatchHooks


def _run_parallel_submission(
    *,
    queue_item: ParallelQueueItem,
    action: PlannedAction,
    market: MarketConfig,
    program: ProgramConfig,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
    reservation_coordinator: AssetReservationCoordinator,
    wallet_id: str,
    hooks: StrategyDispatchHooks,
    queued_at_monotonic: float,
) -> StrategyActionItem:
    submit_index = queue_item.submit_index
    requested_amounts = dict(queue_item.requested_amounts)
    available_amounts = dict(queue_item.available_amounts)
    queue_wait_ms = int((time.monotonic() - queued_at_monotonic) * 1000)
    _log_market_decision(
        str(market.market_id),
        "parallel_offer_queue_wait",
        submit_index=submit_index,
        size=action.size,
        side=planned_action_side(action),
        queue_wait_ms=queue_wait_ms,
    )
    acquire_started = time.monotonic()
    acquired = reservation_coordinator.try_acquire(
        market_id=str(market.market_id),
        wallet_id=wallet_id,
        requested_amounts=requested_amounts,
        available_amounts=available_amounts,
    )
    acquire_ms = int((time.monotonic() - acquire_started) * 1000)
    if not acquired.ok or not acquired.reservation_id:
        return managed_skip_item(
            action=action,
            reason=str(acquired.error or "reservation_rejected"),
        ).with_extra(
            queue_wait_ms=queue_wait_ms,
            reservation_acquire_ms=acquire_ms,
        )
    reservation_id = str(acquired.reservation_id)
    reserved_at = time.monotonic()
    _log_market_decision(
        str(market.market_id),
        "parallel_offer_reservation_acquired",
        submit_index=submit_index,
        reservation_id=reservation_id,
        queue_wait_ms=queue_wait_ms,
        reservation_acquire_ms=acquire_ms,
    )
    try:
        item = hooks.execute_managed_action_with_retry(
            program=program,
            market=market,
            action=action,
            publish_venue=publish_venue,
            runtime_dry_run=runtime_dry_run,
            dexie=dexie,
            execute_single_managed_action=hooks.execute_single_managed_action,
            managed_offer_post=hooks.managed_offer_post,
        )
    except Exception as exc:
        item = parallel_offer_worker_error_item(action=action, exc=exc)
    release_status = reservation_release_status(is_executed=item.counts_as_executed)
    reservation_coordinator.release(reservation_id=reservation_id, status=release_status)
    reservation_hold_ms = int((time.monotonic() - reserved_at) * 1000)
    _log_market_decision(
        str(market.market_id),
        "parallel_offer_reservation_released",
        submit_index=submit_index,
        reservation_id=reservation_id,
        release_status=release_status,
        reservation_hold_ms=reservation_hold_ms,
    )
    return item.with_extra(
        reservation_id=reservation_id,
        queue_wait_ms=queue_wait_ms,
        reservation_acquire_ms=acquire_ms,
        reservation_hold_ms=reservation_hold_ms,
    )


def _run_parallel_batch_submissions(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    expanded_actions: list[PlannedAction],
    batch_plan: ParallelBatchPlan,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
    reservation_coordinator: AssetReservationCoordinator,
    wallet_id: str,
    hooks: StrategyDispatchHooks,
) -> list[StrategyActionItem]:
    if not batch_plan.queue:
        return []

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

    submitted_items: list[tuple[int, StrategyActionItem]] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
        future_to_submission: dict[concurrent.futures.Future[StrategyActionItem], int] = {}
        for queue_item in batch_plan.queue:
            future = pool.submit(
                _run_parallel_submission,
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
        for future in concurrent.futures.as_completed(future_to_submission):
            submit_index = future_to_submission[future]
            try:
                item = future.result()
            except Exception as exc:
                item = parallel_offer_worker_error_item(
                    action=expanded_actions[submit_index],
                    exc=exc,
                )
            submitted_items.append((submit_index, item))

    items: list[StrategyActionItem] = []
    for _, item in sorted(submitted_items, key=lambda pair: pair[0]):
        _log_offer_action_timing(str(market.market_id), item)
        items.append(item)

    _, _, cooldown_seconds = _post_retry_config()
    transient_parallel_failures = count_parallel_transient_failures(
        [item for _submit_idx, item in submitted_items]
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
    return items


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
) -> StrategyActionResult:
    items: list[StrategyActionItem] = []
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
        items.extend(
            _run_parallel_batch_submissions(
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
        )

    return StrategyActionResult.from_items(
        planned_count=len(expanded_actions),
        action_items=items,
    )

"""Parallel managed-offer worker: reservation acquire, post, release."""

from __future__ import annotations

import time

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.cycle import reservation_release_status
from greenfloor.core.parallel_batch_plan import ParallelQueueItem
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.market_helpers import _normalize_offer_side
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.daemon.strategy_dispatch.items import (
    managed_skip_item,
    parallel_offer_worker_error_item,
)
from greenfloor.daemon.strategy_dispatch.runtime import StrategyDispatchHooks


def run_parallel_submission(
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
        side=_normalize_offer_side(action.side),
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
        item = hooks.managed_action_with_retry(
            program=program,
            market=market,
            action=action,
            publish_venue=publish_venue,
            runtime_dry_run=runtime_dry_run,
            dexie=dexie,
        )
    except Exception as exc:
        item = parallel_offer_worker_error_item(exc=exc)
    release_status = reservation_release_status(is_executed=item.is_executed)
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

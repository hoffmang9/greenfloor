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
    reservation_release_status,
    should_apply_parallel_transient_cooldown,
)
from greenfloor.core.cycle_orchestration import ParallelActionOutcome
from greenfloor.core.parallel_batch_plan import ParallelSubmissionEntry
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
from greenfloor.daemon.strategy_dispatch.parallel_batch import (
    PlannedParallelSubmission,
    build_parallel_dispatch_plan,
)
from greenfloor.daemon.strategy_dispatch.reservation_helpers import (
    reservation_request_for_action,
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
    fee_amount_mojos = 0
    wallet_id = reservation_wallet_id(program)
    reservation_coordinator.probe_storage()

    pending_requests: list[tuple[int, dict[str, int]]] = []
    all_asset_ids: set[str] = set()
    for submit_index, action in enumerate(expanded_actions):
        requested_amounts = reservation_request_for_action(
            market=market,
            action=action,
            resolved_base_asset_id=resolved_base_asset_id,
            resolved_quote_asset_id=resolved_quote_asset_id,
            fee_asset_id=resolved_xch_asset_id,
            fee_amount_mojos=fee_amount_mojos,
        )
        all_asset_ids.update(requested_amounts.keys())
        pending_requests.append((submit_index, requested_amounts))

    spendable_profiles = coinset_spendable_profiles_by_asset(
        program=program,
        market=market,
        asset_ids=all_asset_ids,
    )
    batch_entries = [
        ParallelSubmissionEntry(
            submit_index=submit_index,
            requested_amounts=requested_amounts,
            spendable_profiles=spendable_profiles,
        )
        for submit_index, requested_amounts in pending_requests
    ]
    dispatch_plan = build_parallel_dispatch_plan(
        expanded_actions=expanded_actions,
        entries=batch_entries,
    )
    items.extend(dispatch_plan.skip_items)
    submissions = dispatch_plan.submissions

    if not submissions:
        return strategy_action_result(
            planned_count=len(expanded_actions),
            executed_count=executed_count,
            items=items,
        )

    max_workers = parallel_max_workers(
        submission_count=len(submissions),
        configured_max=int(program.runtime_offer_parallelism_max_workers),
    )
    _log_market_decision(
        str(market.market_id),
        "parallel_offer_dispatch",
        planned_count=len(expanded_actions),
        queued_count=len(submissions),
        workers=max_workers,
    )

    def run_parallel_submission(
        submission: PlannedParallelSubmission,
        *,
        queued_at_monotonic: float,
    ) -> StrategyActionItem:
        submit_index = submission.submit_index
        action = submission.action
        requested_amounts = submission.requested_amounts
        available_amounts = submission.available_amounts
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

    with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
        future_to_submission: dict[concurrent.futures.Future[StrategyActionItem], int] = {}
        for submission in submissions:
            future = pool.submit(
                run_parallel_submission,
                submission,
                queued_at_monotonic=time.monotonic(),
            )
            future_to_submission[future] = submission.submit_index
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

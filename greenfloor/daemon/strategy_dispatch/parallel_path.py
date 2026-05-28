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
    plan_parallel_submission_batch,
    reservation_release_status,
    should_apply_parallel_transient_cooldown,
)
from greenfloor.daemon.cooldowns import _POST_COOLDOWN_UNTIL, _post_retry_config, _set_cooldown
from greenfloor.daemon.inventory_scan import _coinset_spendable_profiles_by_asset
from greenfloor.daemon.market_helpers import _normalize_offer_side
from greenfloor.daemon.market_logging import _log_market_decision, _log_offer_action_timing
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.daemon.strategy_dispatch.items import (
    managed_skip_item,
    parallel_offer_worker_error_item,
    strategy_action_result,
)
from greenfloor.daemon.strategy_dispatch.reservation_helpers import (
    reservation_request_for_action,
    reservation_wallet_id,
)


def execute_actions_parallel(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    expanded_actions: list[Any],
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
    reservation_coordinator: AssetReservationCoordinator,
) -> dict[str, Any]:
    items: list[StrategyActionItem] = []
    executed_count = 0
    from greenfloor.daemon import strategy_dispatch as dispatch_pkg

    resolved_base_asset_id, resolved_quote_asset_id, resolved_xch_asset_id = (
        dispatch_pkg._resolve_signer_offer_asset_ids_for_reservation(
            program=program,
            market=market,
        )
    )
    fee_amount_mojos = 0
    wallet_id = reservation_wallet_id(program)
    reservation_coordinator.probe_storage()

    batch_entries: list[dict[str, Any]] = []
    for submit_index, action in enumerate(expanded_actions):
        requested_amounts = reservation_request_for_action(
            market=market,
            action=action,
            resolved_base_asset_id=resolved_base_asset_id,
            resolved_quote_asset_id=resolved_quote_asset_id,
            fee_asset_id=resolved_xch_asset_id,
            fee_amount_mojos=fee_amount_mojos,
        )
        spendable_profiles = _coinset_spendable_profiles_by_asset(
            program=program,
            market=market,
            asset_ids=set(requested_amounts.keys()),
        )
        batch_entries.append(
            {
                "submit_index": submit_index,
                "size": int(getattr(action, "size", 0)),
                "side": _normalize_offer_side(getattr(action, "side", "sell")),
                "requested_amounts": requested_amounts,
                "spendable_profiles": spendable_profiles,
            }
        )

    plan = plan_parallel_submission_batch(batch_entries)
    submissions: list[tuple[int, Any, dict[str, int], dict[str, int]]] = []
    for skip in plan.get("skip_items", []):
        submit_index = int(skip["submit_index"])
        items.append(
            managed_skip_item(
                action=expanded_actions[submit_index],
                reason=str(skip.get("reason", "skipped")),
            )
        )
    for queue in plan.get("queue", []):
        submit_index = int(queue["submit_index"])
        submissions.append(
            (
                submit_index,
                expanded_actions[submit_index],
                {
                    str(asset_id): int(amount)
                    for asset_id, amount in dict(queue.get("requested_amounts", {})).items()
                },
                {
                    str(asset_id): int(amount)
                    for asset_id, amount in dict(queue.get("available_amounts", {})).items()
                },
            )
        )

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
        *,
        submit_index: int,
        action: Any,
        requested_amounts: dict[str, int],
        available_amounts: dict[str, int],
        queued_at_monotonic: float,
    ) -> StrategyActionItem:
        queue_wait_ms = int((time.monotonic() - queued_at_monotonic) * 1000)
        _log_market_decision(
            str(market.market_id),
            "parallel_offer_queue_wait",
            submit_index=submit_index,
            size=int(getattr(action, "size", 0)),
            side=_normalize_offer_side(getattr(action, "side", "sell")),
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
            from greenfloor.daemon import strategy_dispatch as dispatch_pkg

            item = dispatch_pkg._execute_managed_action_with_retry(
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
        for submit_index, action, requested_amounts, available_amounts in submissions:
            future = pool.submit(
                run_parallel_submission,
                submit_index=submit_index,
                action=action,
                requested_amounts=requested_amounts,
                available_amounts=available_amounts,
                queued_at_monotonic=time.monotonic(),
            )
            future_to_submission[future] = submit_index
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
            {
                "status": item.status,
                "transient_upstream": item.transient_upstream,
            }
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

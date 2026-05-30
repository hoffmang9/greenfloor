"""Cycle policy PyO3 bridge wrappers (orchestration + managed dispatch)."""

from __future__ import annotations

from typing import Any

from greenfloor.core.cycle_orchestration import (
    MarketBatchSelection,
    OfferStateRow,
    StaleSweepCandidate,
    StaleSweepHit,
    StaleSweepProgress,
)
from greenfloor.core.engine_bridge import policy_engine
from greenfloor.core.engine_maps import require_i64_i64_map, require_side_offer_count_maps
from greenfloor.core.managed_action_outcome import ManagedActionOutcome
from greenfloor.core.managed_retry import ManagedRetryDecision
from greenfloor.core.parallel_batch_plan import ParallelBatchPlan
from greenfloor.core.parallel_reservation_context import ParallelReservationContext
from greenfloor.core.planned_action import PlannedAction, planned_actions_from_signer_list
from greenfloor.core.strategy_action_item import StrategyActionItem

__all__ = [
    "aggregate_two_sided_offer_counts",
    "apply_offer_signal",
    "can_parallelize_managed_offers",
    "classify_dexie_stale_offer_status",
    "classify_dexie_visibility_outcome",
    "classify_managed_post_result",
    "classify_managed_transient_error",
    "collect_stale_sweep_candidates",
    "count_parallel_transient_failures",
    "dedupe_sorted_market_ids",
    "enqueue_immediate_requeue",
    "evaluate_market",
    "evaluate_two_sided_market_actions",
    "executed_sell_offer_counts_by_size",
    "expand_planned_actions",
    "expiry_seconds_for_action",
    "filter_planned_actions_with_positive_repeat",
    "is_dexie_offer_missing_error_text",
    "is_managed_upstream_transient_error",
    "is_managed_worker_transient_error",
    "is_parallel_dispatch_transient_error",
    "is_transient_dexie_visibility_404_error",
    "is_transient_managed_upstream_error_text",
    "is_two_sided_market_mode",
    "managed_retry_decision",
    "needs_inventory_fallback",
    "next_disabled_market_log_deadline",
    "one_sided_offer_counts_by_side",
    "parallel_max_workers",
    "plan_parallel_managed_dispatch",
    "plan_reseed_actions_from_gap",
    "record_stale_sweep_check",
    "reseed_skip_reason_labels",
    "reservation_release_status",
    "resolve_inventory_scan_source",
    "resolve_tracked_sizes",
    "select_market_batch",
    "should_apply_parallel_transient_cooldown",
    "should_log_disabled_market",
    "should_try_cat_inventory_fallback",
    "should_use_market_slot_dispatch",
    "single_input_preferred_skip_reason",
]


def _normalize_spendable_profiles(
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> dict[str, dict[str, int | bool]]:
    return {
        str(asset_id): {
            "total": int(profile.get("total", 0)),
            "max_single": int(profile.get("max_single", 0)),
            "max_single_known": bool(profile.get("max_single_known", False)),
        }
        for asset_id, profile in spendable_profiles.items()
    }


def evaluate_market(*, state: Any, config: Any) -> list[PlannedAction]:
    signer = policy_engine()
    return planned_actions_from_signer_list(signer.evaluate_market(state, config))


def evaluate_two_sided_market_actions(
    *,
    buy_state: Any,
    sell_state: Any,
    buy_config: Any,
    sell_config: Any,
) -> list[PlannedAction]:
    signer = policy_engine()
    return planned_actions_from_signer_list(
        signer.evaluate_two_sided_market_actions(
            buy_state,
            sell_state,
            buy_config,
            sell_config,
        )
    )


def reseed_skip_reason_labels() -> tuple[str, ...]:
    return tuple(str(label) for label in policy_engine().reseed_skip_reason_labels())


def plan_reseed_actions_from_gap(
    *,
    strategy_actions: list[PlannedAction],
    active_counts_by_size: dict[int, int],
    target_counts_by_size: dict[int, int],
    strategy_config: Any,
    xch_price_usd: float | None,
) -> Any:
    signer = policy_engine()
    return signer.plan_reseed_actions_from_gap(
        strategy_actions,
        active_counts_by_size,
        target_counts_by_size,
        strategy_config,
        xch_price_usd,
    )


def apply_offer_signal(*, state: str, signal: str) -> dict[str, Any]:
    signer = policy_engine()
    result = signer.apply_offer_signal(state, signal)
    if not isinstance(result, dict):
        raise TypeError("apply_offer_signal returned non-dict result")
    return dict(result)


def expiry_seconds_for_action(*, expiry_unit: str, expiry_value: int) -> int | None:
    signer = policy_engine()
    return signer.expiry_seconds_for_action(expiry_unit, expiry_value)


def select_market_batch(
    *,
    enabled_market_ids: list[str],
    slot_count: int,
    cursor: int,
    immediate_requeue_ids: list[str],
) -> MarketBatchSelection:
    signer = policy_engine()
    result = signer.select_market_batch(
        enabled_market_ids,
        int(slot_count),
        int(cursor),
        immediate_requeue_ids,
    )
    if not isinstance(result, MarketBatchSelection):
        raise TypeError("select_market_batch returned non-MarketBatchSelection result")
    return result


def enqueue_immediate_requeue(
    immediate_requeue_ids: list[str],
    market_id: str,
) -> list[str]:
    return list(policy_engine().enqueue_immediate_requeue(immediate_requeue_ids, market_id))


def should_use_market_slot_dispatch(*, enabled_market_count: int, slot_count: int) -> bool:
    return bool(
        policy_engine().should_use_market_slot_dispatch(
            int(enabled_market_count),
            int(slot_count),
        )
    )


def dedupe_sorted_market_ids(market_ids: list[str]) -> list[str]:
    return list(policy_engine().dedupe_sorted_market_ids(market_ids))


def should_log_disabled_market(*, now_monotonic: float, next_log_deadline: float) -> bool:
    return bool(
        policy_engine().should_log_disabled_market(float(now_monotonic), float(next_log_deadline))
    )


def next_disabled_market_log_deadline(*, now_monotonic: float, interval_seconds: int) -> float:
    return float(
        policy_engine().next_disabled_market_log_deadline(
            float(now_monotonic),
            int(interval_seconds),
        )
    )


def should_try_cat_inventory_fallback(*, coinset_scan_empty: bool, base_asset: str) -> bool:
    return bool(
        policy_engine().should_try_cat_inventory_fallback(bool(coinset_scan_empty), base_asset)
    )


def collect_stale_sweep_candidates(
    *,
    rows: list[OfferStateRow],
    enabled_market_ids: list[str],
    per_market_limit: int,
) -> list[StaleSweepCandidate]:
    signer = policy_engine()
    result = signer.collect_stale_sweep_candidates(rows, enabled_market_ids, int(per_market_limit))
    if not isinstance(result, list):
        raise TypeError("collect_stale_sweep_candidates returned non-list result")
    for index, item in enumerate(result):
        if not isinstance(item, StaleSweepCandidate):
            raise TypeError(
                f"stale sweep candidate list item {index} must be StaleSweepCandidate, "
                f"got {type(item).__name__}"
            )
    return result


def classify_dexie_stale_offer_status(status: int) -> str | None:
    return policy_engine().classify_dexie_stale_offer_status(int(status))


def is_dexie_offer_missing_error_text(error_text: str) -> bool:
    return bool(policy_engine().is_dexie_offer_missing_error_text(error_text))


def record_stale_sweep_check(
    *,
    progress: StaleSweepProgress,
    hit: StaleSweepHit | None,
) -> StaleSweepProgress:
    signer = policy_engine()
    result = signer.record_stale_sweep_check(progress, hit)
    if not isinstance(result, StaleSweepProgress):
        raise TypeError("record_stale_sweep_check returned non-StaleSweepProgress result")
    return result


def needs_inventory_fallback(*, bucket_counts_available: bool, coinset_scan_empty: bool) -> bool:
    return bool(
        policy_engine().needs_inventory_fallback(
            bool(bucket_counts_available),
            bool(coinset_scan_empty),
        )
    )


def resolve_inventory_scan_source(
    coinset_scan_found_coins: bool,
    coinset_scan_empty: bool,
    cat_scan_found_coins: bool,
    wallet_scan_found_coins: bool,
) -> str:
    return str(
        policy_engine().resolve_inventory_scan_source(
            bool(coinset_scan_found_coins),
            bool(coinset_scan_empty),
            bool(cat_scan_found_coins),
            bool(wallet_scan_found_coins),
        )
    )


def resolve_tracked_sizes(ladder_sizes: list[int], strategy_default_sizes: list[int]) -> list[int]:
    return [
        int(size)
        for size in policy_engine().resolve_tracked_sizes(
            [int(value) for value in ladder_sizes],
            [int(value) for value in strategy_default_sizes],
        )
    ]


def is_two_sided_market_mode(market_mode: str) -> bool:
    return bool(policy_engine().is_two_sided_market_mode(str(market_mode)))


def aggregate_two_sided_offer_counts(
    buy_counts: dict[int, int],
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> dict[int, int]:
    signer = policy_engine()
    result = signer.aggregate_two_sided_offer_counts(
        buy_counts,
        sell_counts,
        [int(size) for size in tracked_sizes],
    )
    return require_i64_i64_map(result, label="aggregate_two_sided_offer_counts")


def one_sided_offer_counts_by_side(
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> dict[str, dict[int, int]]:
    signer = policy_engine()
    result = signer.one_sided_offer_counts_by_side(
        sell_counts, [int(size) for size in tracked_sizes]
    )
    return require_side_offer_count_maps(
        result,
        label="one_sided_offer_counts_by_side",
    )


def executed_sell_offer_counts_by_size(action_items: list[Any]) -> dict[int, int]:
    result = policy_engine().executed_sell_offer_counts_by_size(action_items)
    return require_i64_i64_map(result, label="executed_sell_offer_counts_by_size")


def expand_planned_actions(actions: list[PlannedAction]) -> list[PlannedAction]:
    signer = policy_engine()
    return planned_actions_from_signer_list(signer.expand_planned_actions(actions))


def filter_planned_actions_with_positive_repeat(
    actions: list[PlannedAction],
) -> list[PlannedAction]:
    signer = policy_engine()
    return planned_actions_from_signer_list(
        signer.filter_planned_actions_with_positive_repeat(actions)
    )


def plan_parallel_managed_dispatch(
    *,
    actions: list[PlannedAction],
    ctx: ParallelReservationContext,
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> ParallelBatchPlan:
    signer = policy_engine()
    result = signer.plan_parallel_managed_dispatch(
        actions,
        ctx,
        _normalize_spendable_profiles(spendable_profiles),
    )
    if not isinstance(result, ParallelBatchPlan):
        raise TypeError("plan_parallel_managed_dispatch returned non-ParallelBatchPlan result")
    return result


def single_input_preferred_skip_reason(
    *,
    requested_amounts: dict[str, int],
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> str | None:
    signer = policy_engine()
    return signer.single_input_preferred_skip_reason(
        requested_amounts,
        _normalize_spendable_profiles(spendable_profiles),
    )


def is_transient_managed_upstream_error_text(error_text: str) -> bool:
    return bool(policy_engine().is_transient_managed_upstream_error_text(error_text))


def classify_managed_transient_error(*, exception_class: str, error_text: str) -> str | None:
    return policy_engine().classify_managed_transient_error(exception_class, error_text)


def is_managed_upstream_transient_error(*, exception_class: str, error_text: str) -> bool:
    return bool(policy_engine().is_managed_upstream_transient_error(exception_class, error_text))


def is_managed_worker_transient_error(*, exception_class: str, error_text: str) -> bool:
    return bool(policy_engine().is_managed_worker_transient_error(exception_class, error_text))


def is_parallel_dispatch_transient_error(*, exception_class: str, error_text: str) -> bool:
    return bool(policy_engine().is_parallel_dispatch_transient_error(exception_class, error_text))


def is_transient_dexie_visibility_404_error(error: str) -> bool:
    return bool(policy_engine().is_transient_dexie_visibility_404_error(error))


def can_parallelize_managed_offers(
    *,
    signer_path_configured: bool,
    parallelism_enabled: bool,
    runtime_dry_run: bool,
    has_coordinator: bool,
) -> bool:
    return bool(
        policy_engine().can_parallelize_managed_offers(
            signer_path_configured,
            parallelism_enabled,
            runtime_dry_run,
            has_coordinator,
        )
    )


def parallel_max_workers(*, submission_count: int, configured_max: int) -> int:
    return int(policy_engine().parallel_max_workers(int(submission_count), int(configured_max)))


def reservation_release_status(*, is_executed: bool) -> str:
    return str(policy_engine().reservation_release_status(bool(is_executed)))


def should_apply_parallel_transient_cooldown(
    *,
    transient_failures: int,
    total_parallel: int,
    cooldown_seconds: int,
) -> bool:
    return bool(
        policy_engine().should_apply_parallel_transient_cooldown(
            int(transient_failures),
            int(total_parallel),
            int(cooldown_seconds),
        )
    )


def managed_retry_decision(
    *,
    attempt_index: int,
    attempts_max: int,
    backoff_ms: int,
    is_upstream_transient: bool,
) -> ManagedRetryDecision:
    signer = policy_engine()
    result = signer.managed_retry_decision(
        int(attempt_index),
        int(attempts_max),
        int(backoff_ms),
        bool(is_upstream_transient),
    )
    if not isinstance(result, ManagedRetryDecision):
        raise TypeError("managed_retry_decision returned non-ManagedRetryDecision result")
    return result


def classify_managed_post_result(
    *,
    success: bool,
    error_text: str,
    offer_id: str,
    publish_venue: str,
) -> ManagedActionOutcome:
    signer = policy_engine()
    result = signer.classify_managed_post_result(success, error_text, offer_id, publish_venue)
    if not isinstance(result, ManagedActionOutcome):
        raise TypeError("classify_managed_post_result returned non-ManagedActionOutcome result")
    return result


def classify_dexie_visibility_outcome(
    *,
    visible: bool,
    visibility_error: str,
) -> ManagedActionOutcome:
    signer = policy_engine()
    result = signer.classify_dexie_visibility_outcome(visible, visibility_error)
    if not isinstance(result, ManagedActionOutcome):
        raise TypeError(
            "classify_dexie_visibility_outcome returned non-ManagedActionOutcome result"
        )
    return result


def count_parallel_transient_failures(items: list[StrategyActionItem]) -> int:
    for index, item in enumerate(items):
        if not isinstance(item, StrategyActionItem):
            raise TypeError(
                f"parallel outcome list item {index} must be StrategyActionItem, "
                f"got {type(item).__name__}"
            )
    return int(policy_engine().count_parallel_transient_failures(items))

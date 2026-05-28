"""PyO3 bridge for the Rust daemon cycle kernel (re-exports by domain)."""

from __future__ import annotations

from greenfloor.core.cycle import _bridge_managed as managed
from greenfloor.core.cycle import _bridge_orchestration as orchestration

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
    "sequential_action_route",
    "should_apply_parallel_transient_cooldown",
    "should_log_disabled_market",
    "should_try_cat_inventory_fallback",
    "should_use_market_slot_dispatch",
    "single_input_preferred_skip_reason",
]

aggregate_two_sided_offer_counts = orchestration.aggregate_two_sided_offer_counts
apply_offer_signal = orchestration.apply_offer_signal
can_parallelize_managed_offers = managed.can_parallelize_managed_offers
classify_dexie_stale_offer_status = orchestration.classify_dexie_stale_offer_status
classify_dexie_visibility_outcome = managed.classify_dexie_visibility_outcome
classify_managed_post_result = managed.classify_managed_post_result
classify_managed_transient_error = managed.classify_managed_transient_error
collect_stale_sweep_candidates = orchestration.collect_stale_sweep_candidates
count_parallel_transient_failures = managed.count_parallel_transient_failures
dedupe_sorted_market_ids = orchestration.dedupe_sorted_market_ids
enqueue_immediate_requeue = orchestration.enqueue_immediate_requeue
evaluate_market = orchestration.evaluate_market
evaluate_two_sided_market_actions = orchestration.evaluate_two_sided_market_actions
expand_planned_actions = managed.expand_planned_actions
expiry_seconds_for_action = orchestration.expiry_seconds_for_action
filter_planned_actions_with_positive_repeat = managed.filter_planned_actions_with_positive_repeat
is_dexie_offer_missing_error_text = orchestration.is_dexie_offer_missing_error_text
is_managed_upstream_transient_error = managed.is_managed_upstream_transient_error
is_managed_worker_transient_error = managed.is_managed_worker_transient_error
is_parallel_dispatch_transient_error = managed.is_parallel_dispatch_transient_error
is_transient_dexie_visibility_404_error = managed.is_transient_dexie_visibility_404_error
is_transient_managed_upstream_error_text = managed.is_transient_managed_upstream_error_text
is_two_sided_market_mode = orchestration.is_two_sided_market_mode
managed_retry_decision = managed.managed_retry_decision
needs_inventory_fallback = orchestration.needs_inventory_fallback
next_disabled_market_log_deadline = orchestration.next_disabled_market_log_deadline
one_sided_offer_counts_by_side = orchestration.one_sided_offer_counts_by_side
parallel_max_workers = managed.parallel_max_workers
plan_parallel_managed_dispatch = managed.plan_parallel_managed_dispatch
plan_reseed_actions_from_gap = orchestration.plan_reseed_actions_from_gap
record_stale_sweep_check = orchestration.record_stale_sweep_check
reseed_skip_reason_labels = orchestration.reseed_skip_reason_labels
reservation_release_status = managed.reservation_release_status
resolve_inventory_scan_source = orchestration.resolve_inventory_scan_source
resolve_tracked_sizes = orchestration.resolve_tracked_sizes
select_market_batch = orchestration.select_market_batch
sequential_action_route = managed.sequential_action_route
should_apply_parallel_transient_cooldown = managed.should_apply_parallel_transient_cooldown
should_log_disabled_market = orchestration.should_log_disabled_market
should_try_cat_inventory_fallback = orchestration.should_try_cat_inventory_fallback
should_use_market_slot_dispatch = orchestration.should_use_market_slot_dispatch
single_input_preferred_skip_reason = managed.single_input_preferred_skip_reason

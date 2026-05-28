"""Direct re-exports from the Rust cycle bridge (no Python shaping)."""

from __future__ import annotations

from greenfloor.core.kernel_bridge import import_kernel

from ._bridge_managed import (
    can_parallelize_managed_offers,
    classify_dexie_visibility_outcome,
    classify_managed_post_result,
    count_parallel_transient_failures,
    expand_planned_actions,
    filter_planned_actions_with_positive_repeat,
    is_managed_upstream_transient_error,
    is_managed_worker_transient_error,
    is_parallel_dispatch_transient_error,
    is_transient_dexie_visibility_404_error,
    is_transient_managed_upstream_error_text,
    managed_retry_decision,
    parallel_max_workers,
    plan_parallel_managed_dispatch,
    reservation_release_status,
    sequential_action_route,
    should_apply_parallel_transient_cooldown,
    single_input_preferred_skip_reason,
)
from ._bridge_orchestration import (
    classify_dexie_stale_offer_status,
    collect_stale_sweep_candidates,
    dedupe_sorted_market_ids,
    enqueue_immediate_requeue,
    evaluate_market,
    evaluate_two_sided_market_actions,
    is_dexie_offer_missing_error_text,
    is_two_sided_market_mode,
    needs_inventory_fallback,
    next_disabled_market_log_deadline,
    record_stale_sweep_check,
    select_market_batch,
    should_log_disabled_market,
    should_try_cat_inventory_fallback,
    should_use_market_slot_dispatch,
)


def market_cycle_phases() -> tuple[str, ...]:
    return tuple(import_kernel().market_cycle_phases())


MARKET_CYCLE_PHASES: tuple[str, ...] = market_cycle_phases()

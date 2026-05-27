pub mod dispatch;
pub mod lifecycle;
pub mod managed;
pub mod market;
pub mod orchestration;
pub mod strategy;

pub use dispatch::{
    expand_strategy_actions, expiry_seconds_for_action, reservation_request_for_managed_offer,
    single_input_preferred_skip_reason, PlannedActionInput, SpendableAssetProfile,
};
pub use lifecycle::{apply_offer_signal, OfferLifecycleState, OfferSignal, OfferTransition};
pub use managed::{
    can_parallelize_managed_offers, classify_dexie_visibility_outcome,
    classify_managed_post_result, classify_managed_transient_error,
    count_parallel_transient_failures, is_managed_upstream_transient_error,
    is_managed_worker_transient_error, is_parallel_dispatch_transient_error,
    is_transient_dexie_visibility_404_error, is_transient_managed_upstream_error_text,
    managed_retry_sleep_ms, parallel_max_workers, prepare_parallel_managed_submission_decision,
    reservation_release_status, should_apply_parallel_transient_cooldown, should_retry_managed_post,
    ManagedActionOutcome, ParallelSubmissionDecision,
};
pub use market::{
    aggregate_two_sided_offer_counts, is_two_sided_market_mode, market_cycle_phases,
    needs_inventory_fallback, one_sided_offer_counts_by_side, resolve_inventory_scan_source,
    resolve_tracked_sizes, wallet_fallback_source_label, MarketCyclePhase, MarketCycleResultState,
};
pub use orchestration::{
    classify_dexie_stale_offer_status, collect_stale_sweep_candidates,
    dedupe_sorted_market_ids, enqueue_immediate_requeue, is_dexie_offer_missing_error_text,
    next_disabled_market_log_deadline, record_stale_sweep_check, select_market_batch,
    should_log_disabled_market, should_try_cat_inventory_fallback, should_use_market_slot_dispatch,
    MarketBatchSelection, OfferStateRow, StaleSweepCandidate, StaleSweepHit, StaleSweepProgress,
    MARKET_CYCLE_PHASES,
};
pub use strategy::{evaluate_market, MarketState, PlannedAction, StrategyConfig};

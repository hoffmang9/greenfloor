//! Deterministic cycle orchestration, managed dispatch, and offer reconciliation.
//!
//! Deterministic daemon cycle policy.

pub mod cancel;
pub mod dispatch;
pub mod execution;
pub mod lifecycle;
pub mod managed;
pub mod market;
pub mod notifications;
pub mod orchestration;
pub mod reconcile;
pub mod reseed;
pub mod retry;
pub mod strategy;
pub mod strategy_action;

pub use cancel::{
    abs_move_bps, cancel_move_threshold_bps, collect_open_offer_ids_for_cancel,
    evaluate_cancel_policy_decision, CancelPolicyDecision,
};
pub use dispatch::{
    expiry_seconds_for_action, reservation_request_for_managed_offer,
    single_input_preferred_skip_reason, ManagedOfferReservationRequest, PlannedActionInput,
    SpendableAssetProfile,
};
pub use execution::{
    expand_planned_actions, filter_planned_actions_with_positive_repeat,
    plan_parallel_managed_dispatch, ParallelBatchPlan, ParallelQueueItem,
    ParallelReservationContext, ParallelSkipItem,
};
pub use lifecycle::{apply_offer_signal, OfferLifecycleState, OfferSignal, OfferTransition};
pub use managed::{
    can_parallelize_managed_offers, classify_dexie_visibility_outcome,
    classify_managed_post_result, classify_managed_transient_error,
    count_parallel_transient_failures, is_managed_upstream_transient_error,
    is_managed_worker_transient_error, is_parallel_dispatch_transient_error,
    is_transient_dexie_visibility_404_error, is_transient_managed_upstream_error_text,
    managed_retry_decision, parallel_managed_dispatch_enabled, parallel_max_workers,
    prepare_parallel_managed_submission_decision, reservation_release_status,
    should_apply_parallel_transient_cooldown, ManagedActionOutcome, ManagedActionStatus,
    ManagedRetryDecision, ManagedRetryDecisionKind, ParallelSubmissionDecision,
};
pub use market::{
    aggregate_two_sided_offer_counts, is_two_sided_market_mode, market_cycle_phases,
    needs_inventory_fallback, one_sided_offer_counts_by_side, post_reconcile_market_cycle_phases,
    resolve_inventory_scan_source, resolve_tracked_sizes, wallet_fallback_source_label,
    MarketCyclePhase, MarketCycleResultState,
};
pub use notifications::{
    evaluate_low_inventory_alert, AlertEvent, AlertState, LowInventoryEvaluation, LowInventoryInput,
};
pub use orchestration::{
    classify_dexie_stale_offer_status, collect_stale_sweep_candidates, dedupe_sorted_market_ids,
    enqueue_immediate_requeue, is_dexie_offer_missing_error_text,
    next_disabled_market_log_deadline, record_stale_sweep_check, select_market_batch,
    should_log_disabled_market, should_try_cat_inventory_fallback, should_use_market_slot_dispatch,
    MarketBatchSelection, OfferStateRow, StaleSweepCandidate, StaleSweepHit, StaleSweepProgress,
    DEFAULT_DISABLED_MARKET_LOG_INTERVAL_SECONDS, MIN_DISABLED_MARKET_LOG_INTERVAL_SECONDS,
};
pub use reconcile::{
    resolve_missing_watched_offer_transition, resolve_watched_offer_transition_from_signals,
    unchanged_offer_transition, unsupported_venue_offer_transition, CycleOfferTransition,
};
pub use reseed::{
    plan_reseed_actions_from_gap, reseed_skip_reason_labels, ReseedGapPlan, ReseedSkipReason,
};
pub use retry::{
    coinset_fee_lookup_retry_sleep, dexie_invalid_offer_retry_sleep,
    dexie_invalid_offer_should_retry, moderate_retry_next_sleep, moderate_retry_sleep_seconds,
    parse_rate_limit_retry_seconds, poll_exponential_advance_sleep, poll_exponential_sleep_now,
};
pub use strategy::{
    evaluate_market, evaluate_two_sided_market_actions, MarketState, PlannedAction, StrategyConfig,
};
pub use strategy_action::{executed_sell_offer_counts_by_size, StrategyActionSellCountInput};

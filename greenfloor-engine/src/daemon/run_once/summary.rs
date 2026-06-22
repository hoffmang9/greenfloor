use std::time::Instant;

use crate::cycle::dedupe_sorted_market_ids;
use crate::metrics::metric_millis_to_u64;

use super::cycle_types::{CyclePlan, DaemonCycleSummary, MarketDispatchMetrics};

#[must_use]
pub fn build_cycle_summary(
    plan: &CyclePlan,
    metrics: &MarketDispatchMetrics,
    preamble_error_count: u64,
    duration_ms: u64,
) -> DaemonCycleSummary {
    let immediate_requeue_market_ids =
        dedupe_sorted_market_ids(&metrics.immediate_requeue_market_ids);
    let immediate_requeue_count = immediate_requeue_market_ids.len();
    DaemonCycleSummary {
        duration_ms,
        enabled_markets: plan.enabled_market_ids.len(),
        markets_attempted: plan.selected_market_ids.len(),
        markets_processed: metrics.markets_processed,
        runtime_market_slot_count: plan.configured_market_slot_count,
        stale_open_sweep_checked_offer_count: plan.stale_open_sweep.checked_offer_count as u64,
        stale_open_sweep_requeue_market_ids: plan.stale_open_sweep.requeue_market_ids.clone(),
        stale_open_sweep_requeue_count: plan.stale_open_sweep.requeue_market_ids.len(),
        stale_open_sweep_truncated: plan.stale_open_sweep.truncated,
        immediate_requeue_market_ids,
        immediate_requeue_count,
        error_count: preamble_error_count + metrics.cycle_error_count,
        strategy_planned_total: metrics.strategy_planned_total,
        strategy_executed_total: metrics.strategy_executed_total,
        cancel_triggered_count: metrics.cancel_triggered_count,
        cancel_planned_total: metrics.cancel_planned_total,
        cancel_executed_total: metrics.cancel_executed_total,
        consumed_immediate_requeues: plan.consumed_immediate_requeues.clone(),
    }
}

#[must_use]
pub fn cycle_started_instant() -> Instant {
    Instant::now()
}

#[must_use]
pub fn elapsed_ms(started: Instant) -> u64 {
    metric_millis_to_u64(started.elapsed().as_millis())
}

#[must_use]
pub fn compute_cycle_exit_code(plan: &CyclePlan, metrics: &MarketDispatchMetrics) -> i32 {
    let attempted = plan.selected_market_ids.len();
    if attempted > 0 && metrics.markets_processed == 0 {
        return 1;
    }
    0
}

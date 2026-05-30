use crate::error::SignerResult;
use crate::cycle::MarketCycleResultState;

use super::reconcile_phase::ReconcilePhaseResult;
use super::run_once::MarketDispatchMetrics;

#[derive(Debug, Clone)]
pub struct SingleMarketCycleOutput {
    pub market_id: String,
    pub reconcile: ReconcilePhaseResult,
    pub state: MarketCycleResultState,
}

pub fn aggregate_market_dispatch_metrics(
    outputs: &[SingleMarketCycleOutput],
) -> MarketDispatchMetrics {
    let mut metrics = MarketDispatchMetrics::default();
    metrics.markets_processed = outputs.len() as u64;
    for output in outputs {
        metrics.cycle_error_count += output.reconcile.metrics.cycle_errors;
        metrics.cycle_error_count += output.state.cycle_errors.max(0) as u64;
        metrics.strategy_planned_total += output.state.strategy_planned.max(0) as u64;
        metrics.strategy_executed_total += output.state.strategy_executed.max(0) as u64;
        if output.state.cancel_triggered {
            metrics.cancel_triggered_count += 1;
        }
        metrics.cancel_planned_total += output.state.cancel_planned.max(0) as u64;
        metrics.cancel_executed_total += output.state.cancel_executed.max(0) as u64;
        if output.reconcile.metrics.immediate_requeue_requested {
            metrics
                .immediate_requeue_market_ids
                .push(output.market_id.clone());
        }
    }
    metrics
}

pub fn record_market_worker_error(
    store: &crate::storage::SqliteStore,
    market_id: &str,
    error: &str,
    source: &str,
) -> SignerResult<()> {
    store.add_audit_event(
        "market_cycle_error",
        &serde_json::json!({
            "market_id": market_id,
            "error": error,
            "source": source,
        }),
        None,
    )
}

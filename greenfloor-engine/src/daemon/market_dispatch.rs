use crate::adapters::DexieClient;
use crate::error::SignerResult;

use super::cancel_phase::CancelPhaseMetrics;
use super::market_context::MarketDispatchContext;
use super::reconcile_phase::ReconcilePhaseResult;
use super::run_once::MarketDispatchMetrics;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct IoPhaseMetrics {
    pub cycle_error_count: u64,
    pub strategy_planned_total: u64,
    pub strategy_executed_total: u64,
}

#[derive(Debug, Clone)]
pub struct SingleMarketCycleOutput {
    pub market_id: String,
    pub reconcile: ReconcilePhaseResult,
    pub io: IoPhaseMetrics,
    pub cancel: CancelPhaseMetrics,
    pub immediate_requeue_requested: bool,
}

pub fn aggregate_market_dispatch_metrics(
    outputs: &[SingleMarketCycleOutput],
) -> MarketDispatchMetrics {
    let mut metrics = MarketDispatchMetrics::default();
    metrics.markets_processed = outputs.len() as u64;
    for output in outputs {
        metrics.cycle_error_count += output.reconcile.metrics.cycle_errors;
        metrics.cycle_error_count += output.io.cycle_error_count;
        metrics.strategy_planned_total += output.io.strategy_planned_total;
        metrics.strategy_executed_total += output.io.strategy_executed_total;
        if output.cancel.cancel_triggered {
            metrics.cancel_triggered_count += 1;
        }
        metrics.cancel_planned_total += output.cancel.cancel_planned;
        metrics.cancel_executed_total += output.cancel.cancel_executed;
        if output.immediate_requeue_requested {
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

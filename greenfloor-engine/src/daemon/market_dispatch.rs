use crate::cycle::MarketCycleResultState;
use crate::error::SignerResult;

use super::reconcile_market_cycle::ReconcileMarketCycleResult;
use super::run_once::MarketDispatchMetrics;

#[derive(Debug, Clone)]
pub struct SingleMarketCycleOutput {
    pub market_id: String,
    pub reconcile: ReconcileMarketCycleResult,
    pub state: MarketCycleResultState,
}

pub fn aggregate_market_dispatch_metrics(
    outputs: &[SingleMarketCycleOutput],
) -> MarketDispatchMetrics {
    let mut metrics = MarketDispatchMetrics {
        markets_processed: outputs.len().try_into().unwrap_or(0u64),
        ..Default::default()
    };
    for output in outputs {
        metrics.cycle_error_count += output.reconcile.metrics.cycle_errors;
        metrics.cycle_error_count += output.state.cycle_errors.max(0).try_into().unwrap_or(0u64);
        metrics.strategy_planned_total += output
            .state
            .strategy_planned
            .max(0)
            .try_into()
            .unwrap_or(0u64);
        metrics.strategy_executed_total += output
            .state
            .strategy_executed
            .max(0)
            .try_into()
            .unwrap_or(0u64);
        if output.state.cancel_triggered {
            metrics.cancel_triggered_count += 1;
        }
        metrics.cancel_planned_total += output
            .state
            .cancel_planned
            .max(0)
            .try_into()
            .unwrap_or(0u64);
        metrics.cancel_executed_total += output
            .state
            .cancel_executed
            .max(0)
            .try_into()
            .unwrap_or(0u64);
        if output.state.immediate_requeue_requested {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::reconcile_market_cycle::ReconcileMarketCycleMetrics;

    fn sample_output(immediate_requeue: bool) -> SingleMarketCycleOutput {
        let mut state = MarketCycleResultState::default();
        if immediate_requeue {
            state.request_immediate_requeue(Some("taker_fill".to_string()));
        }
        SingleMarketCycleOutput {
            market_id: "m1".to_string(),
            reconcile: ReconcileMarketCycleResult {
                offers: Vec::new(),
                dexie_size_by_offer_id: std::collections::HashMap::default(),
                dexie_fetch_error: None,
                metrics: ReconcileMarketCycleMetrics::default(),
            },
            state,
        }
    }

    #[test]
    fn aggregate_metrics_uses_cycle_state_immediate_requeue() {
        let metrics =
            aggregate_market_dispatch_metrics(&[sample_output(false), sample_output(true)]);
        assert_eq!(metrics.immediate_requeue_market_ids, vec!["m1".to_string()]);
    }
}

use crate::cycle::MarketCycleResultState;
use crate::error::SignerResult;
use crate::operator_log::{audit_market_cycle, MARKET_CYCLE_ERROR};
use tracing::Level;

use crate::metrics::{metric_collection_len_to_u64, metric_non_negative_u64};

use super::reconcile_market_cycle::ReconcileMarketCycleResult;
use super::run_once::MarketDispatchMetrics;

#[derive(Debug, Clone)]
pub struct SingleMarketCycleOutput {
    pub market_id: String,
    pub reconcile: ReconcileMarketCycleResult,
    pub state: MarketCycleResultState,
}

#[must_use]
pub fn aggregate_market_dispatch_metrics(
    outputs: &[SingleMarketCycleOutput],
) -> MarketDispatchMetrics {
    let mut metrics = MarketDispatchMetrics {
        markets_processed: metric_collection_len_to_u64(outputs.len()),
        ..Default::default()
    };
    for output in outputs {
        metrics.cycle_error_count += output.reconcile.metrics.cycle_errors;
        metrics.cycle_error_count += metric_non_negative_u64(output.state.cycle_errors);
        metrics.strategy_planned_total += metric_non_negative_u64(output.state.strategy_planned);
        metrics.strategy_executed_total += metric_non_negative_u64(output.state.strategy_executed);
        if output.state.cancel_triggered {
            metrics.cancel_triggered_count += 1;
        }
        metrics.cancel_planned_total += metric_non_negative_u64(output.state.cancel_planned);
        metrics.cancel_executed_total += metric_non_negative_u64(output.state.cancel_executed);
        if output.state.immediate_requeue_requested {
            metrics
                .immediate_requeue_market_ids
                .push(output.market_id.clone());
        }
    }
    metrics
}

/// Record market worker error.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn record_market_worker_error(
    store: &crate::storage::SqliteStore,
    market_id: &str,
    error: &str,
    source: &str,
) -> SignerResult<()> {
    audit_market_cycle(
        store,
        Level::WARN,
        MARKET_CYCLE_ERROR,
        &serde_json::json!({
            "market_id": market_id,
            "error": error,
            "source": source,
        }),
        market_id,
        "market cycle worker error",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::reconcile_market_cycle::ReconcileMarketCycleMetrics;
    use crate::operator_log::{TraceCapture, MARKET_CYCLE_ERROR};

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

    #[test]
    fn record_market_worker_error_dual_emits_audit_and_trace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = crate::storage::SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let capture = TraceCapture::install();

        record_market_worker_error(&store, "m1", "forced failure", "test").expect("record");

        let events = store
            .list_recent_audit_events(Some(&[MARKET_CYCLE_ERROR]), Some("m1"), 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert!(capture.logs().contains(MARKET_CYCLE_ERROR));
        assert!(capture.logs().contains("forced failure"));
    }
}

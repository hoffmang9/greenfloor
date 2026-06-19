use std::time::Instant;

use crate::config::MarketConfig;
use crate::cycle::enqueue_immediate_requeue;
use crate::error::SignerResult;
use crate::operator_log::{
    audit_daemon_cycle, LogContext, DAEMON_CYCLE_COMPLETED, DAEMON_CYCLE_STARTED,
    DAEMON_CYCLE_SUMMARY,
};
use crate::storage::SqliteStore;
use tracing::Level;

use super::market_context::{
    load_cycle_resources, DaemonCycleResources, MarketCycleContext, MarketDispatchContext,
};
use super::market_cycle::run_post_reconcile_market_phases;
use super::market_dispatch::{
    aggregate_market_dispatch_metrics, record_market_worker_error, SingleMarketCycleOutput,
};
use super::preamble::run_cycle_preamble;
use super::reconcile_augment::merge_reconcile_immediate_requeue;
use super::reconcile_market_cycle::run_reconcile_market_cycle;
use super::run_once::{
    build_cycle_plan, build_cycle_summary, compute_cycle_exit_code, cycle_started_instant,
    elapsed_ms, CyclePlan, DaemonCycleSummary, DaemonDispatchState, DaemonRunOnceRequest,
    MarketDispatchMetrics,
};
use crate::storage::resolve_state_db_path;

/// Daemon cycles always process markets sequentially on one `SQLite` store.
pub const SEQUENTIAL_MARKET_WORKER_SOURCE: &str = "sequential_market_worker";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DaemonCycleOnceResponse {
    pub exit_code: i32,
    pub dispatch_state: DaemonDispatchState,
    pub cycle_summary: DaemonCycleSummary,
}

fn write_stale_sweep_audit(store: &SqliteStore, plan: &CyclePlan) -> SignerResult<()> {
    if plan.stale_open_sweep.requeue_market_ids.is_empty() {
        return Ok(());
    }
    store.add_audit_event(
        "stale_open_offer_requeue_detected",
        &serde_json::json!({
            "market_ids": plan.stale_open_sweep.requeue_market_ids,
            "checked_offer_count": plan.stale_open_sweep.checked_offer_count,
            "truncated": plan.stale_open_sweep.truncated,
            "hits": plan.stale_open_sweep.hits,
        }),
        None,
    )
}

async fn process_one_market(
    store: &SqliteStore,
    resources: &DaemonCycleResources,
    dispatch_context: &MarketDispatchContext,
    plan: &CyclePlan,
    market: &MarketConfig,
) -> SignerResult<SingleMarketCycleOutput> {
    let reconcile = run_reconcile_market_cycle(
        store,
        &resources.coin_watchlist,
        &resources.dexie,
        market,
        &resources.network,
    )
    .await?;
    let phase_context = MarketCycleContext {
        resources,
        dispatch: dispatch_context,
        plan,
        reconcile: &reconcile,
    };
    let mut state = run_post_reconcile_market_phases(store, &phase_context, market).await?;
    merge_reconcile_immediate_requeue(&mut state, &reconcile.metrics);

    Ok(SingleMarketCycleOutput {
        market_id: market.market_id.clone(),
        reconcile,
        state,
    })
}

fn record_market_result(
    error_store: &SqliteStore,
    market_id: &str,
    result: SignerResult<SingleMarketCycleOutput>,
    source: &str,
) -> SignerResult<Result<SingleMarketCycleOutput, u64>> {
    match result {
        Ok(output) => Ok(Ok(output)),
        Err(err) => {
            record_market_worker_error(error_store, market_id, &err.to_string(), source)?;
            Ok(Err(1))
        }
    }
}

async fn dispatch_markets(
    cycle_store: &SqliteStore,
    resources: &DaemonCycleResources,
    dispatch_context: &MarketDispatchContext,
    plan: &CyclePlan,
    markets: Vec<MarketConfig>,
) -> SignerResult<(Vec<SingleMarketCycleOutput>, u64)> {
    let mut worker_errors = 0u64;
    let mut outputs = Vec::with_capacity(markets.len());
    for market in markets {
        let result =
            process_one_market(cycle_store, resources, dispatch_context, plan, &market).await;
        match record_market_result(
            cycle_store,
            &market.market_id,
            result,
            SEQUENTIAL_MARKET_WORKER_SOURCE,
        )? {
            Ok(output) => outputs.push(output),
            Err(count) => worker_errors += count,
        }
    }
    Ok((outputs, worker_errors))
}

/// Run daemon cycle once.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_daemon_cycle_once(
    request: &DaemonRunOnceRequest,
) -> SignerResult<DaemonCycleOnceResponse> {
    let started: Instant = cycle_started_instant();
    let resources = load_cycle_resources(request)?;
    super::disabled_markets::log_disabled_markets_periodic(&resources.markets);

    let db_path = resolve_state_db_path(
        &resources.program().home_dir,
        request.state_db_override.as_deref(),
    );
    let cycle_store = SqliteStore::open(&db_path)?;
    let plan = build_cycle_plan(request, &resources, &cycle_store).await?;
    write_stale_sweep_audit(&cycle_store, &plan)?;

    crate::trace_event!(
        INFO,
        LogContext::DAEMON_CYCLE,
        DAEMON_CYCLE_STARTED,
        {
            market_count = plan.selected_market_ids.len(),
            dry_run = plan.runtime_dry_run,
            selected_market_ids = ?plan.selected_market_ids,
        };
        "daemon cycle started"
    );

    let preamble = run_cycle_preamble(
        resources.program(),
        &cycle_store,
        &request.coinset_base_url,
        &resources.coin_watchlist,
        request.poll_coinset_mempool,
        request.use_websocket_capture,
    )
    .await?;

    let dispatch_context = MarketDispatchContext {
        db_path: plan.db_path.clone(),
        allowed_key_ids: request.allowed_key_ids.clone(),
        xch_price_usd: preamble.xch_price_usd,
        previous_xch_price_usd: plan.previous_xch_price_usd,
        runtime_dry_run: plan.runtime_dry_run,
        test_controls: plan.test_controls.clone(),
    };
    let markets = resources.selected_markets(&plan.selected_market_ids);

    let (cycle_outputs, worker_errors) =
        dispatch_markets(&cycle_store, &resources, &dispatch_context, &plan, markets).await?;

    let mut metrics: MarketDispatchMetrics = aggregate_market_dispatch_metrics(&cycle_outputs);
    metrics.cycle_error_count += worker_errors;
    let mut dispatch_state = plan.dispatch_state.clone();
    for market_id in &metrics.immediate_requeue_market_ids {
        dispatch_state.immediate_requeue_ids =
            enqueue_immediate_requeue(&dispatch_state.immediate_requeue_ids, market_id);
    }

    let summary = build_cycle_summary(
        &plan,
        &metrics,
        preamble.cycle_error_count,
        elapsed_ms(started),
    );
    let summary_payload = serde_json::to_value(&summary).map_err(|err| {
        crate::error::SignerError::Other(format!("failed to encode daemon_cycle_summary: {err}"))
    })?;
    audit_daemon_cycle(
        &cycle_store,
        Level::INFO,
        DAEMON_CYCLE_SUMMARY,
        &summary_payload,
        "daemon cycle summary",
    )?;

    let exit_code = compute_cycle_exit_code(&plan, &metrics);
    trace_daemon_cycle_completed(exit_code, &summary, plan.selected_market_ids.len());

    Ok(DaemonCycleOnceResponse {
        exit_code,
        dispatch_state,
        cycle_summary: summary,
    })
}

fn trace_daemon_cycle_completed(exit_code: i32, summary: &DaemonCycleSummary, market_count: usize) {
    let level = if exit_code == 0 {
        Level::INFO
    } else {
        Level::WARN
    };
    crate::event_at_level!(
        level,
        service = LogContext::DAEMON_CYCLE.service,
        event = DAEMON_CYCLE_COMPLETED,
        phase = LogContext::DAEMON_CYCLE.phase,
        exit_code = exit_code,
        cycle_error_count = summary.error_count,
        elapsed_ms = summary.duration_ms,
        market_count = market_count,
        "daemon cycle completed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operator_log::TraceCapture;

    #[test]
    fn market_dispatch_is_sequential_on_one_sqlite_connection() {
        assert_eq!(SEQUENTIAL_MARKET_WORKER_SOURCE, "sequential_market_worker");
    }

    #[test]
    fn daemon_cycle_completed_emits_single_trace_event() {
        let capture = TraceCapture::install();
        let summary = DaemonCycleSummary {
            duration_ms: 42,
            enabled_markets: 1,
            markets_attempted: 1,
            markets_processed: 0,
            runtime_market_slot_count: 1,
            stale_open_sweep_checked_offer_count: 0,
            stale_open_sweep_requeue_market_ids: Vec::new(),
            stale_open_sweep_requeue_count: 0,
            stale_open_sweep_truncated: false,
            immediate_requeue_market_ids: Vec::new(),
            immediate_requeue_count: 0,
            error_count: 2,
            strategy_planned_total: 0,
            strategy_executed_total: 0,
            cancel_triggered_count: 0,
            cancel_planned_total: 0,
            cancel_executed_total: 0,
            consumed_immediate_requeues: Vec::new(),
        };
        trace_daemon_cycle_completed(2, &summary, 1);
        assert_eq!(capture.count_substr(DAEMON_CYCLE_COMPLETED), 1);
        assert!(capture.logs().contains("cycle_error_count"));
    }
}

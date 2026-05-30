use std::time::Instant;

use crate::config::MarketConfig;
use crate::cycle::enqueue_immediate_requeue;
use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

use super::market_context::{
    load_cycle_resources, DaemonCycleResources, MarketCycleContext, MarketDispatchContext,
};
use super::market_cycle::run_post_reconcile_market_phases;
use super::market_dispatch::{
    aggregate_market_dispatch_metrics, record_market_worker_error, IoPhaseMetrics,
    SingleMarketCycleOutput,
};
use super::preamble::run_cycle_preamble;
use super::reconcile_phase::run_market_reconcile_phase;
use super::run_once::{
    build_cycle_plan, build_cycle_summary, compute_cycle_exit_code, cycle_started_instant,
    elapsed_ms, CyclePlan, DaemonCycleSummary, DaemonDispatchState, DaemonRunOnceRequest,
    MarketDispatchMetrics,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct DaemonCycleOnceResponse {
    pub exit_code: i32,
    pub dispatch_state: DaemonDispatchState,
    pub cycle_summary: DaemonCycleSummary,
}

fn write_stale_sweep_audit(store_path: &std::path::Path, plan: &CyclePlan) -> SignerResult<()> {
    if plan.stale_open_sweep.requeue_market_ids.is_empty() {
        return Ok(());
    }
    let store = SqliteStore::open(store_path)?;
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
    resources: &DaemonCycleResources,
    dispatch_context: &MarketDispatchContext,
    plan: &CyclePlan,
    market: &MarketConfig,
) -> SignerResult<SingleMarketCycleOutput> {
    let store = SqliteStore::open(&plan.db_path)?;
    let reconcile =
        run_market_reconcile_phase(&store, &resources.dexie, market, &resources.network).await?;
    let phase_context = MarketCycleContext {
        resources,
        dispatch: dispatch_context,
        plan,
        reconcile: &reconcile,
    };
    let phases = run_post_reconcile_market_phases(&store, &phase_context, market).await?;
    let immediate_requeue_requested = reconcile.metrics.immediate_requeue_requested;
    let io = IoPhaseMetrics {
        cycle_error_count: phases.metrics.cycle_error_count,
        strategy_planned_total: phases.metrics.strategy_planned_total,
        strategy_executed_total: phases.metrics.strategy_executed_total,
    };

    Ok(SingleMarketCycleOutput {
        market_id: market.market_id.clone(),
        reconcile,
        io,
        cancel: phases.cancel,
        immediate_requeue_requested,
    })
}

async fn dispatch_markets(
    resources: &DaemonCycleResources,
    dispatch_context: &MarketDispatchContext,
    plan: &CyclePlan,
    markets: Vec<MarketConfig>,
) -> SignerResult<(Vec<SingleMarketCycleOutput>, u64)> {
    let parallel = dispatch_context.parallel_markets_enabled && markets.len() > 1;
    let error_store = SqliteStore::open(&plan.db_path)?;
    let mut worker_errors = 0u64;

    if parallel {
        // rusqlite connections are !Send; run each market on a blocking thread with an
        // isolated current-thread runtime so the SQLite handle can cross internal awaits.
        let mut blocking_tasks = Vec::with_capacity(markets.len());
        for market in markets {
            let resources = resources.clone();
            let dispatch_context = dispatch_context.clone();
            let plan = plan.clone();
            let market_id = market.market_id.clone();
            blocking_tasks.push(tokio::task::spawn_blocking(move || {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("market worker runtime")
                    .block_on(process_one_market(
                        &resources,
                        &dispatch_context,
                        &plan,
                        &market,
                    ));
                (market_id, result)
            }));
        }
        let mut outputs = Vec::new();
        for task in blocking_tasks {
            let (market_id, result) = task
                .await
                .map_err(|err| SignerError::Other(format!("parallel market join failed: {err}")))?;
            match result {
                Ok(output) => outputs.push(output),
                Err(err) => {
                    record_market_worker_error(
                        &error_store,
                        &market_id,
                        &err.to_string(),
                        "parallel_market_worker",
                    )?;
                    worker_errors += 1;
                }
            }
        }
        return Ok((outputs, worker_errors));
    }

    let mut outputs = Vec::with_capacity(markets.len());
    for market in markets {
        match process_one_market(resources, dispatch_context, plan, &market).await {
            Ok(output) => outputs.push(output),
            Err(err) => {
                record_market_worker_error(
                    &error_store,
                    &market.market_id,
                    &err.to_string(),
                    "sequential_market_worker",
                )?;
                worker_errors += 1;
            }
        }
    }
    Ok((outputs, worker_errors))
}

async fn run_daemon_cycle_once_inner(
    request: &DaemonRunOnceRequest,
) -> SignerResult<DaemonCycleOnceResponse> {
    let started: Instant = cycle_started_instant();
    let resources = load_cycle_resources(request)?;
    let plan = build_cycle_plan(request, &resources).await?;
    write_stale_sweep_audit(&plan.db_path, &plan)?;

    let preamble = run_cycle_preamble(
        &resources.program,
        &plan.db_path,
        &request.coinset_base_url,
        request.poll_coinset_mempool,
        request.use_websocket_capture,
    )
    .await?;

    let dispatch_context = MarketDispatchContext {
        db_path: plan.db_path.clone(),
        state_dir: request.state_dir.clone(),
        allowed_key_ids: request.allowed_key_ids.clone(),
        xch_price_usd: preamble.xch_price_usd,
        previous_xch_price_usd: plan.previous_xch_price_usd,
        parallel_markets_enabled: plan.parallel_markets_enabled,
        runtime_dry_run: plan.runtime_dry_run,
        test_controls: plan.test_controls.clone(),
    };
    let markets = resources.selected_markets(&plan.selected_market_ids);

    let (cycle_outputs, worker_errors) =
        dispatch_markets(&resources, &dispatch_context, &plan, markets).await?;

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
    let summary_store = SqliteStore::open(&plan.db_path)?;
    let summary_payload = serde_json::to_value(&summary).map_err(|err| {
        SignerError::Other(format!("failed to encode daemon_cycle_summary: {err}"))
    })?;
    summary_store.add_audit_event("daemon_cycle_summary", &summary_payload, None)?;

    Ok(DaemonCycleOnceResponse {
        exit_code: compute_cycle_exit_code(&plan, &metrics),
        dispatch_state,
        cycle_summary: summary,
    })
}

pub async fn run_daemon_cycle_once(
    request: &DaemonRunOnceRequest,
) -> SignerResult<DaemonCycleOnceResponse> {
    Box::pin(run_daemon_cycle_once_inner(request)).await
}

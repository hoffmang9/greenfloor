use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::config::MarketConfig;
use crate::cycle::enqueue_immediate_requeue;
use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

use super::market_dispatch::{
    aggregate_market_dispatch_metrics, dexie_client, io_metrics_from_value, market_bridge_kwargs,
    program_network, record_market_worker_error, run_market_cancel_phase_for_market,
    run_market_reconcile_phase_for_market, selected_markets, MarketDispatchContext,
    SingleMarketCycleOutput,
};
use super::python_bridge::DaemonPythonBridge;
use super::run_once::{
    build_cycle_plan, build_cycle_summary, cycle_started_instant, elapsed_ms, CyclePlan,
    DaemonDispatchState, DaemonRunOnceRequest, MarketDispatchMetrics,
};

#[derive(Debug, Clone)]
pub struct DaemonCycleOnceResponse {
    pub exit_code: i32,
    pub dispatch_state: DaemonDispatchState,
    pub cycle_summary: Value,
}

async fn call_python_bridge(
    bridge: Arc<dyn DaemonPythonBridge>,
    method: &str,
    kwargs: Value,
) -> SignerResult<Value> {
    let method = method.to_string();
    tokio::task::spawn_blocking(move || bridge.call_method(&method, &kwargs))
        .await
        .map_err(|err| SignerError::Other(format!("python bridge task failed: {err}")))?
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
    request: &DaemonRunOnceRequest,
    plan: &CyclePlan,
    dispatch_context: &MarketDispatchContext,
    network: &str,
    dexie: &crate::adapters::DexieClient,
    market: &MarketConfig,
    bridge: Arc<dyn DaemonPythonBridge>,
) -> SignerResult<SingleMarketCycleOutput> {
    let store = SqliteStore::open(&plan.db_path)?;
    let reconcile =
        run_market_reconcile_phase_for_market(&store, dexie, market, network).await?;

    let io_value = call_python_bridge(
        Arc::clone(&bridge),
        "run_market_cycle_python_phases",
        market_bridge_kwargs(
            request,
            plan,
            market,
            &reconcile,
            dispatch_context.xch_price_usd,
        ),
    )
    .await?;
    let io_metrics = io_metrics_from_value(&io_value)?;

    let immediate_requeue_requested = reconcile.metrics.immediate_requeue_requested;
    let cancel = run_market_cancel_phase_for_market(
        &store,
        dexie,
        market,
        &reconcile.offers,
        plan.runtime_dry_run,
        dispatch_context.xch_price_usd,
        plan.previous_xch_price_usd,
    )
    .await?;

    Ok(SingleMarketCycleOutput {
        market_id: market.market_id.clone(),
        reconcile,
        io: io_metrics,
        cancel,
        immediate_requeue_requested,
    })
}

async fn dispatch_markets(
    request: &DaemonRunOnceRequest,
    plan: &CyclePlan,
    dispatch_context: &MarketDispatchContext,
    network: String,
    dexie: crate::adapters::DexieClient,
    markets: Vec<MarketConfig>,
    bridge: Arc<dyn DaemonPythonBridge>,
) -> SignerResult<(Vec<SingleMarketCycleOutput>, u64)> {
    let parallel =
        dispatch_context.parallel_markets_enabled && markets.len() > 1 && bridge.supports_parallel_workers();
    let error_store = SqliteStore::open(&plan.db_path)?;
    let mut worker_errors = 0u64;
    if parallel {
        let mut tasks = tokio::task::JoinSet::new();
        for market in markets {
            let request = request.clone();
            let plan = plan.clone();
            let dispatch_context = dispatch_context.clone();
            let network = network.clone();
            let dexie = dexie.clone();
            let bridge = Arc::clone(&bridge);
            let market_id = market.market_id.clone();
            tasks.spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("market worker runtime")
                        .block_on(process_one_market(
                            &request,
                            &plan,
                            &dispatch_context,
                            &network,
                            &dexie,
                            &market,
                            bridge,
                        ))
                })
                .await;
                (market_id, result)
            });
        }
        let mut outputs = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            let (market_id, worker_result) = joined.map_err(|err| {
                crate::error::SignerError::Other(format!("parallel market task failed: {err}"))
            })?;
            let blocking_result = worker_result.map_err(|err| {
                crate::error::SignerError::Other(format!("parallel market join failed: {err}"))
            })?;
            match blocking_result {
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
        match process_one_market(
            request,
            plan,
            dispatch_context,
            &network,
            &dexie,
            &market,
            Arc::clone(&bridge),
        )
        .await
        {
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

pub async fn run_daemon_cycle_once(
    request: &DaemonRunOnceRequest,
    bridge: Arc<dyn DaemonPythonBridge>,
) -> SignerResult<DaemonCycleOnceResponse> {
    let started: Instant = cycle_started_instant();
    let plan = build_cycle_plan(request).await?;
    write_stale_sweep_audit(&plan.db_path, &plan)?;

    let preamble_kwargs = serde_json::json!({
        "program_path": request.program_path,
        "db_path": plan.db_path,
        "coinset_base_url": request.coinset_base_url,
        "poll_coinset_mempool": request.poll_coinset_mempool,
        "use_websocket_capture": request.use_websocket_capture,
    });
    let preamble = call_python_bridge(bridge.clone(), "run_cycle_preamble", preamble_kwargs).await?;
    let preamble_error_count = preamble
        .get("cycle_error_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let xch_price_usd = preamble.get("xch_price_usd").and_then(Value::as_f64);

    let dispatch_context = MarketDispatchContext {
        program_path: request.program_path.clone(),
        markets_path: request.markets_path.clone(),
        testnet_markets_path: request.testnet_markets_path.clone(),
        db_path: plan.db_path.clone(),
        state_dir: request.state_dir.clone(),
        selected_market_ids: plan.selected_market_ids.clone(),
        allowed_key_ids: request.allowed_key_ids.clone(),
        xch_price_usd,
        previous_xch_price_usd: plan.previous_xch_price_usd,
        parallel_markets_enabled: plan.parallel_markets_enabled,
        runtime_dry_run: plan.runtime_dry_run,
    };
    let network = program_network(&dispatch_context)?;
    let markets = selected_markets(&dispatch_context)?;
    let dexie = dexie_client(&dispatch_context)?;

    let (cycle_outputs, worker_errors) = dispatch_markets(
        request,
        &plan,
        &dispatch_context,
        network,
        dexie,
        markets,
        bridge,
    )
    .await?;

    let mut metrics: MarketDispatchMetrics = aggregate_market_dispatch_metrics(&cycle_outputs);
    metrics.cycle_error_count += worker_errors;
    let mut dispatch_state = plan.dispatch_state.clone();
    for market_id in &metrics.immediate_requeue_market_ids {
        dispatch_state.immediate_requeue_ids =
            enqueue_immediate_requeue(&dispatch_state.immediate_requeue_ids, market_id);
    }

    let summary = build_cycle_summary(&plan, &metrics, preamble_error_count, elapsed_ms(started));
    let summary_store = SqliteStore::open(&plan.db_path)?;
    summary_store.add_audit_event("daemon_cycle_summary", &summary, None)?;

    Ok(DaemonCycleOnceResponse {
        exit_code: 0,
        dispatch_state,
        cycle_summary: summary,
    })
}

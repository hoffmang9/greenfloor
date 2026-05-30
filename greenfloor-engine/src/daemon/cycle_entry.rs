use std::time::Instant;

use serde_json::Value;

use crate::config::{load_program_config, ManagerProgramConfig, MarketConfig};
use crate::cycle::enqueue_immediate_requeue;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::cancel_phase::run_market_cancel_phase;
use super::market_dispatch::{
    aggregate_market_dispatch_metrics, dexie_client, program_network, record_market_worker_error,
    selected_markets, IoPhaseMetrics, MarketDispatchContext, SingleMarketCycleOutput,
};
use super::market_phases::run_market_phases;
use super::preamble::run_cycle_preamble;
use super::reconcile_phase::run_market_reconcile_phase;
use super::run_once::{
    build_cycle_plan, build_cycle_summary, compute_cycle_exit_code, cycle_started_instant,
    elapsed_ms, CyclePlan, DaemonDispatchState, DaemonRunOnceRequest, MarketDispatchMetrics,
};

#[derive(Debug, Clone)]
pub struct DaemonCycleOnceResponse {
    pub exit_code: i32,
    pub dispatch_state: DaemonDispatchState,
    pub cycle_summary: Value,
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
    _request: &DaemonRunOnceRequest,
    plan: &CyclePlan,
    program: &ManagerProgramConfig,
    dispatch_context: &MarketDispatchContext,
    network: &str,
    dexie: &crate::adapters::DexieClient,
    market: &MarketConfig,
) -> SignerResult<SingleMarketCycleOutput> {
    let store = SqliteStore::open(&plan.db_path)?;
    let reconcile = run_market_reconcile_phase(&store, dexie, market, network).await?;

    let phase_metrics = run_market_phases(
        &store,
        program,
        market,
        network,
        &dispatch_context.program_path,
        &dispatch_context.markets_path,
        dispatch_context.testnet_markets_path.as_deref(),
        &reconcile,
        dispatch_context.xch_price_usd,
    )
    .await?;
    let io = IoPhaseMetrics {
        cycle_error_count: phase_metrics.cycle_error_count,
        strategy_planned_total: phase_metrics.strategy_planned_total,
        strategy_executed_total: phase_metrics.strategy_executed_total,
    };

    let immediate_requeue_requested = reconcile.metrics.immediate_requeue_requested;
    let (cancel, _payload) = run_market_cancel_phase(
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
        io,
        cancel,
        immediate_requeue_requested,
    })
}

async fn dispatch_markets(
    request: &DaemonRunOnceRequest,
    plan: &CyclePlan,
    program: &ManagerProgramConfig,
    dispatch_context: &MarketDispatchContext,
    network: String,
    dexie: crate::adapters::DexieClient,
    markets: Vec<MarketConfig>,
) -> SignerResult<(Vec<SingleMarketCycleOutput>, u64)> {
    let parallel = dispatch_context.parallel_markets_enabled && markets.len() > 1;
    let error_store = SqliteStore::open(&plan.db_path)?;
    let mut worker_errors = 0u64;

    if parallel {
        let mut tasks = tokio::task::JoinSet::new();
        for market in markets {
            let request = request.clone();
            let plan = plan.clone();
            let program = program.clone();
            let dispatch_context = dispatch_context.clone();
            let network = network.clone();
            let dexie = dexie.clone();
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
                            &program,
                            &dispatch_context,
                            &network,
                            &dexie,
                            &market,
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
            program,
            dispatch_context,
            &network,
            &dexie,
            &market,
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
) -> SignerResult<DaemonCycleOnceResponse> {
    let started: Instant = cycle_started_instant();
    let plan = build_cycle_plan(request).await?;
    write_stale_sweep_audit(&plan.db_path, &plan)?;

    let program = load_program_config(&request.program_path)?;
    let preamble = run_cycle_preamble(
        &request.program_path,
        &plan.db_path,
        &request.coinset_base_url,
        request.poll_coinset_mempool,
        request.use_websocket_capture,
    )
    .await?;

    let dispatch_context = MarketDispatchContext {
        program_path: request.program_path.clone(),
        markets_path: request.markets_path.clone(),
        testnet_markets_path: request.testnet_markets_path.clone(),
        db_path: plan.db_path.clone(),
        state_dir: request.state_dir.clone(),
        selected_market_ids: plan.selected_market_ids.clone(),
        allowed_key_ids: request.allowed_key_ids.clone(),
        xch_price_usd: preamble.xch_price_usd,
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
        &program,
        &dispatch_context,
        network,
        dexie,
        markets,
    )
    .await?;

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
    summary_store.add_audit_event("daemon_cycle_summary", &summary, None)?;

    Ok(DaemonCycleOnceResponse {
        exit_code: compute_cycle_exit_code(&plan, &metrics),
        dispatch_state,
        cycle_summary: summary,
    })
}

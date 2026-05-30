use std::time::Instant;

use serde_json::Value;

use crate::cycle::enqueue_immediate_requeue;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::market_dispatch::{
    aggregate_market_dispatch_metrics, dexie_client, load_runtime_dry_run, open_store,
    program_network, reconcile_context_for_python, run_market_cancel_phase_for_market,
    run_market_reconcile_phase_for_market, selected_markets, IoPhaseMetrics, MarketDispatchContext,
    SingleMarketCycleOutput,
};
use super::python_bridge::SubprocessPythonBridge;
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

fn io_metrics_from_value(value: &Value) -> SignerResult<IoPhaseMetrics> {
    serde_json::from_value(value.clone()).map_err(|err| {
        crate::error::SignerError::Other(format!("invalid io phase metrics payload: {err}"))
    })
}

pub async fn run_daemon_cycle_once(
    request: &DaemonRunOnceRequest,
    bridge: &SubprocessPythonBridge,
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
    let preamble = bridge.call_method("run_cycle_preamble", &preamble_kwargs)?;
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
        runtime_dry_run: load_runtime_dry_run(&request.program_path)?,
    };
    let network = program_network(&dispatch_context)?;
    let markets = selected_markets(&dispatch_context)?;
    let dexie = dexie_client(&dispatch_context)?;
    let store = open_store(&plan.db_path)?;

    let mut cycle_outputs: Vec<SingleMarketCycleOutput> = Vec::with_capacity(markets.len());
    for market in markets {
        let reconcile =
            run_market_reconcile_phase_for_market(&store, &dexie, &market, &network).await?;

        let mut io_kwargs = serde_json::json!({
            "program_path": request.program_path,
            "markets_path": request.markets_path,
            "market_id": market.market_id,
            "allowed_key_ids": request.allowed_key_ids,
            "db_path": plan.db_path,
            "state_dir": request.state_dir,
            "xch_price_usd": xch_price_usd,
            "reconcile_context": reconcile_context_for_python(&reconcile),
        });
        if let Some(path) = request.testnet_markets_path.as_ref() {
            io_kwargs["testnet_markets_path"] = Value::String(path.to_string_lossy().into_owned());
        }
        let io_value = bridge.call_method("run_market_cycle_io_phases", &io_kwargs)?;
        let io_metrics = io_metrics_from_value(&io_value)?;

        let cancel = run_market_cancel_phase_for_market(
            &store,
            &dexie,
            &market,
            &reconcile.offers,
            dispatch_context.runtime_dry_run,
            xch_price_usd,
            plan.previous_xch_price_usd,
        )
        .await?;

        let mut coin_ops_kwargs = serde_json::json!({
            "program_path": request.program_path,
            "markets_path": request.markets_path,
            "market_id": market.market_id,
            "allowed_key_ids": request.allowed_key_ids,
            "db_path": plan.db_path,
            "state_dir": request.state_dir,
            "io_context": io_value,
        });
        if let Some(path) = request.testnet_markets_path.as_ref() {
            coin_ops_kwargs["testnet_markets_path"] =
                Value::String(path.to_string_lossy().into_owned());
        }
        bridge.call_method("run_market_coin_ops_phase", &coin_ops_kwargs)?;

        cycle_outputs.push(SingleMarketCycleOutput {
            market_id: market.market_id.clone(),
            reconcile: reconcile.clone(),
            io: io_metrics,
            cancel,
            immediate_requeue_requested: reconcile.metrics.immediate_requeue_requested,
        });
    }

    let metrics: MarketDispatchMetrics = aggregate_market_dispatch_metrics(&cycle_outputs);
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

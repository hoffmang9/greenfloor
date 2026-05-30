use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::adapters::DexieClient;
use crate::config::{load_markets_config_with_overlay, load_program_config, MarketConfig};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::cancel_phase::{run_market_cancel_phase, CancelPhaseMetrics};
use super::reconcile_phase::{run_market_reconcile_phase, ReconcilePhaseMetrics, ReconcilePhaseResult};
use super::run_once::MarketDispatchMetrics;

#[derive(Debug, Clone)]
pub struct MarketDispatchContext {
    pub program_path: std::path::PathBuf,
    pub markets_path: std::path::PathBuf,
    pub testnet_markets_path: Option<std::path::PathBuf>,
    pub db_path: std::path::PathBuf,
    pub state_dir: std::path::PathBuf,
    pub selected_market_ids: Vec<String>,
    pub allowed_key_ids: Vec<String>,
    pub xch_price_usd: Option<f64>,
    pub previous_xch_price_usd: Option<f64>,
    pub parallel_markets_enabled: bool,
    pub runtime_dry_run: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct IoPhaseMetrics {
    pub cycle_error_count: u64,
    pub strategy_planned_total: u64,
    pub strategy_executed_total: u64,
    pub coin_ops_payload: Value,
}

#[derive(Debug, Clone)]
pub struct SingleMarketCycleOutput {
    pub market_id: String,
    pub reconcile: ReconcilePhaseResult,
    pub io: IoPhaseMetrics,
    pub cancel: CancelPhaseMetrics,
    pub immediate_requeue_requested: bool,
}

pub fn load_runtime_dry_run(program_path: &Path) -> SignerResult<bool> {
    let raw = std::fs::read_to_string(program_path).map_err(|err| {
        crate::error::SignerError::Other(format!(
            "failed to read config {}: {err}",
            program_path.display()
        ))
    })?;
    let parsed: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(|err| {
        crate::error::SignerError::Other(format!(
            "failed to parse config {}: {err}",
            program_path.display()
        ))
    })?;
    Ok(parsed
        .get("runtime")
        .and_then(|value| value.get("dry_run"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false))
}

pub fn selected_markets(
    context: &MarketDispatchContext,
) -> SignerResult<Vec<MarketConfig>> {
    let markets = load_markets_config_with_overlay(
        &context.markets_path,
        context.testnet_markets_path.as_deref(),
    )?;
    let selected: std::collections::HashSet<String> = context
        .selected_market_ids
        .iter()
        .map(|market_id| market_id.trim().to_string())
        .filter(|market_id| !market_id.is_empty())
        .collect();
    Ok(markets
        .markets
        .into_iter()
        .filter(|market| market.enabled && selected.contains(&market.market_id))
        .collect())
}

pub async fn run_market_reconcile_phase_for_market(
    store: &SqliteStore,
    dexie: &DexieClient,
    market: &MarketConfig,
    network: &str,
) -> SignerResult<ReconcilePhaseResult> {
    run_market_reconcile_phase(store, dexie, market, network).await
}

pub async fn run_market_cancel_phase_for_market(
    store: &SqliteStore,
    dexie: &DexieClient,
    market: &MarketConfig,
    offers: &[Value],
    runtime_dry_run: bool,
    current_xch_price_usd: Option<f64>,
    previous_xch_price_usd: Option<f64>,
) -> SignerResult<CancelPhaseMetrics> {
    let (metrics, _payload) = run_market_cancel_phase(
        store,
        dexie,
        market,
        offers,
        runtime_dry_run,
        current_xch_price_usd,
        previous_xch_price_usd,
    )
    .await?;
    Ok(metrics)
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

pub fn merge_reconcile_metrics(
    left: &mut ReconcilePhaseMetrics,
    right: &ReconcilePhaseMetrics,
) {
    left.cycle_errors += right.cycle_errors;
    if right.immediate_requeue_requested {
        left.immediate_requeue_requested = true;
    }
    left.immediate_requeue_signals
        .extend(right.immediate_requeue_signals.clone());
}

pub fn program_network(context: &MarketDispatchContext) -> SignerResult<String> {
    Ok(load_program_config(&context.program_path)?.network)
}

pub fn open_store(db_path: &Path) -> SignerResult<SqliteStore> {
    SqliteStore::open(db_path)
}

pub fn dexie_client(context: &MarketDispatchContext) -> SignerResult<DexieClient> {
    let program = load_program_config(&context.program_path)?;
    Ok(DexieClient::new(program.dexie_api_base))
}

pub fn reconcile_context_for_python(result: &ReconcilePhaseResult) -> Value {
    serde_json::json!({
        "offers": result.offers,
        "dexie_size_by_offer_id": result.dexie_size_by_offer_id,
        "dexie_fetch_error": result.dexie_fetch_error,
    })
}

pub fn build_market_lookup(markets: &[MarketConfig]) -> HashMap<String, MarketConfig> {
    markets
        .iter()
        .map(|market| (market.market_id.clone(), market.clone()))
        .collect()
}

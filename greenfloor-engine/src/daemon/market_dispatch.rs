use crate::adapters::DexieClient;
use crate::config::{load_markets_config_with_overlay, load_program_config, MarketConfig};
use crate::error::SignerResult;

use super::cancel_phase::CancelPhaseMetrics;
use super::reconcile_phase::ReconcilePhaseResult;
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
}

#[derive(Debug, Clone)]
pub struct SingleMarketCycleOutput {
    pub market_id: String,
    pub reconcile: ReconcilePhaseResult,
    pub io: IoPhaseMetrics,
    pub cancel: CancelPhaseMetrics,
    pub immediate_requeue_requested: bool,
}

pub fn selected_markets(context: &MarketDispatchContext) -> SignerResult<Vec<MarketConfig>> {
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

pub fn program_network(context: &MarketDispatchContext) -> SignerResult<String> {
    Ok(load_program_config(&context.program_path)?.network)
}

pub fn dexie_client(context: &MarketDispatchContext) -> SignerResult<DexieClient> {
    let program = load_program_config(&context.program_path)?;
    Ok(DexieClient::new(program.dexie_api_base))
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

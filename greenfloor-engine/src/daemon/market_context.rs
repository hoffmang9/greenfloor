use std::path::PathBuf;

use crate::adapters::DexieClient;
use crate::config::{ManagerProgramConfig, MarketConfig, MarketsConfig};
use crate::error::SignerResult;

use super::reconcile_phase::ReconcilePhaseResult;
use super::run_once::{CyclePlan, DaemonRunOnceRequest};

/// Config and clients loaded once per daemon cycle.
#[derive(Debug, Clone)]
pub struct DaemonCycleResources {
    pub program: ManagerProgramConfig,
    pub markets: MarketsConfig,
    pub network: String,
    pub dexie: DexieClient,
    pub program_path: PathBuf,
    pub markets_path: PathBuf,
    pub testnet_markets_path: Option<PathBuf>,
}

impl DaemonCycleResources {
    pub fn selected_markets(&self, selected_market_ids: &[String]) -> Vec<MarketConfig> {
        let selected: std::collections::HashSet<String> = selected_market_ids
            .iter()
            .map(|market_id| market_id.trim().to_string())
            .filter(|market_id| !market_id.is_empty())
            .collect();
        self.markets
            .markets
            .iter()
            .filter(|market| market.enabled && selected.contains(&market.market_id))
            .cloned()
            .collect()
    }
}

/// Shared per-cycle inputs for post-reconcile market phases.
#[derive(Debug, Clone)]
pub struct MarketDispatchContext {
    pub db_path: PathBuf,
    pub allowed_key_ids: Vec<String>,
    pub xch_price_usd: Option<f64>,
    pub previous_xch_price_usd: Option<f64>,
    pub parallel_markets_enabled: bool,
    pub runtime_dry_run: bool,
    pub test_controls: super::run_once::DaemonCycleTestControls,
}

/// Per-market inputs for inventory → strategy → cancel → coin_ops.
#[derive(Debug, Clone)]
pub struct MarketCycleContext<'a> {
    pub resources: &'a DaemonCycleResources,
    pub dispatch: &'a MarketDispatchContext,
    pub plan: &'a CyclePlan,
    pub reconcile: &'a ReconcilePhaseResult,
}

pub fn load_cycle_resources(request: &DaemonRunOnceRequest) -> SignerResult<DaemonCycleResources> {
    let program = crate::config::load_program_config(&request.program_path)?;
    let markets = crate::config::load_markets_config_with_overlay(
        &request.markets_path,
        request.testnet_markets_path.as_deref(),
    )?;
    super::disabled_markets::log_disabled_markets_startup_once(&markets);
    let network = program.network.clone();
    let dexie = DexieClient::new(program.dexie_api_base.clone());
    Ok(DaemonCycleResources {
        program,
        markets,
        network,
        dexie,
        program_path: request.program_path.clone(),
        markets_path: request.markets_path.clone(),
        testnet_markets_path: request.testnet_markets_path.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn sample_market(market_id: &str, enabled: bool) -> MarketConfig {
        MarketConfig {
            market_id: market_id.to_string(),
            enabled,
            base_asset: "asset1".to_string(),
            base_symbol: "AS1".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "unstable".to_string(),
            receive_address: "xch1test".to_string(),
            signer_key_id: "key-1".to_string(),
            mode: "sell_only".to_string(),
            pricing: json!({}),
            cancel_move_threshold_bps: None,
            ladders: HashMap::new(),
        }
    }

    fn sample_resources(markets: Vec<MarketConfig>) -> DaemonCycleResources {
        DaemonCycleResources {
            program: ManagerProgramConfig {
                network: "mainnet".to_string(),
                home_dir: PathBuf::from("/tmp/gf"),
                app_log_level: "INFO".to_string(),
                app_log_level_was_missing: false,
                dexie_api_base: "https://api.dexie.space".to_string(),
                splash_api_base: "http://localhost:4000".to_string(),
                offer_publish_venue: "dexie".to_string(),
                coin_ops_minimum_fee_mojos: 0,
                coin_ops_max_operations_per_run: 0,
                coin_ops_max_daily_fee_budget_mojos: 0,
                coin_ops_split_fee_mojos: 0,
                coin_ops_combine_fee_mojos: 0,
                runtime_offer_bootstrap_wait_timeout_seconds: 120,
                runtime_market_slot_count: 1,
                runtime_parallel_markets: false,
                runtime_offer_parallelism_enabled: false,
                runtime_offer_parallelism_max_workers: 2,
                runtime_dry_run: false,
                runtime_loop_interval_seconds: 30,
                tx_block_trigger_mode: "websocket".to_string(),
                tx_block_websocket_url: String::new(),
                tx_block_websocket_reconnect_interval_seconds: 1,
                tx_block_fallback_poll_interval_seconds: 1,
            },
            markets: MarketsConfig { markets },
            network: "mainnet".to_string(),
            dexie: DexieClient::new("https://api.dexie.space"),
            program_path: PathBuf::from("/tmp/program.yaml"),
            markets_path: PathBuf::from("/tmp/markets.yaml"),
            testnet_markets_path: None,
        }
    }

    #[test]
    fn selected_markets_filters_disabled_and_unknown_ids() {
        let resources = sample_resources(vec![
            sample_market("enabled", true),
            sample_market("disabled", false),
            sample_market("other", true),
        ]);
        let selected = resources.selected_markets(&["enabled".to_string(), "missing".to_string()]);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].market_id, "enabled");
    }
}

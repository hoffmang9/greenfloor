use std::path::PathBuf;
use std::sync::Arc;

use crate::adapters::DexieClient;
use crate::config::{
    load_markets_config_with_overlay, parse_program_config, read_program_yaml, CycleProgramConfig,
    ManagerProgramConfig, MarketConfig, MarketsConfig, SignerConfig,
};
use crate::error::SignerResult;

use super::cycle_paths::DaemonCyclePaths;
use super::reconcile_market_cycle::ReconcileMarketCycleResult;
use super::run_once::{CyclePlan, DaemonRunOnceRequest};
use super::watchlist::cache::CoinWatchlistCache;

/// Config and clients loaded once per daemon cycle.
#[derive(Debug, Clone)]
pub struct DaemonCycleResources {
    program_config: CycleProgramConfig,
    pub markets: MarketsConfig,
    pub network: String,
    pub dexie: DexieClient,
    pub paths: DaemonCyclePaths,
    pub coin_watchlist: Arc<CoinWatchlistCache>,
}

impl DaemonCycleResources {
    pub fn program_path(&self) -> &std::path::Path {
        &self.paths.program_path
    }

    pub fn program(&self) -> &ManagerProgramConfig {
        self.program_config.program()
    }

    pub fn signer_for_execution(&self) -> SignerResult<&SignerConfig> {
        self.program_config.signer_for_execution()
    }

    pub(crate) fn with_program_config(
        program_config: CycleProgramConfig,
        markets: MarketsConfig,
        network: String,
        dexie: DexieClient,
        paths: DaemonCyclePaths,
        coin_watchlist: Arc<CoinWatchlistCache>,
    ) -> Self {
        Self {
            program_config,
            markets,
            network,
            dexie,
            paths,
            coin_watchlist,
        }
    }

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
    pub runtime_dry_run: bool,
    pub test_controls: super::run_once::DaemonCycleTestControls,
}

/// Per-market inputs for inventory → strategy → cancel → coin_ops.
#[derive(Debug, Clone)]
pub struct MarketCycleContext<'a> {
    pub resources: &'a DaemonCycleResources,
    pub dispatch: &'a MarketDispatchContext,
    pub plan: &'a CyclePlan,
    pub reconcile: &'a ReconcileMarketCycleResult,
}

pub fn load_cycle_resources(request: &DaemonRunOnceRequest) -> SignerResult<DaemonCycleResources> {
    let raw = read_program_yaml(&request.program_path)?;
    let program = parse_program_config(&raw)?;
    let program_config = CycleProgramConfig::from_parsed(program, &raw);
    let network = program_config.program().network.clone();
    let dexie_api_base = program_config.program().dexie_api_base.clone();
    let markets = load_markets_config_with_overlay(
        &request.markets_path,
        request.testnet_markets_path.as_deref(),
    )?;
    super::disabled_markets::log_disabled_markets_startup_once(&markets);
    let dexie = DexieClient::new(dexie_api_base);
    let coin_watchlist = request.coin_watchlist.clone();
    Ok(DaemonCycleResources::with_program_config(
        program_config,
        markets,
        network,
        dexie,
        DaemonCyclePaths::new(
            request.program_path.clone(),
            request.markets_path.clone(),
            request.testnet_markets_path.clone(),
        ),
        coin_watchlist,
    ))
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
            ladders: HashMap::default(),
        }
    }

    fn sample_resources(markets: Vec<MarketConfig>) -> DaemonCycleResources {
        DaemonCycleResources::with_program_config(
            CycleProgramConfig::from_parts(
                ManagerProgramConfig {
                    runtime_market_slot_count: 1,
                    runtime_offer_parallelism_max_workers: 2,
                    tx_block_websocket_reconnect_interval_seconds: 1,
                    tx_block_fallback_poll_interval_seconds: 1,
                    ..Default::default()
                },
                None,
            ),
            MarketsConfig { markets },
            "mainnet".to_string(),
            DexieClient::new("https://api.dexie.space"),
            DaemonCyclePaths::new(
                PathBuf::from("/tmp/program.yaml"),
                PathBuf::from("/tmp/markets.yaml"),
                None,
            ),
            CoinWatchlistCache::new(),
        )
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

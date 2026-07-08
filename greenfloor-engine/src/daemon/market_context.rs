use std::sync::Arc;

use crate::adapters::DexieClient;
use crate::config::{
    load_daemon_cycle_config, operator_ticker_index_from_paths, CatTickerIndex, CycleProgramConfig,
    GatedOperatorMarket, ManagerProgramConfig, MarketConfig, MarketsConfig, SignerConfig,
};
use crate::error::SignerResult;
use crate::storage::CycleWriteStore;

use super::cycle_paths::DaemonCyclePaths;
use super::inventory_freshness::InventoryFreshnessCache;
use super::reconcile_market_cycle::ReconcileMarketCycleResult;
use super::run_once::{CyclePlan, DaemonRunOnceRequest};

/// Config and clients loaded once per daemon cycle.
#[derive(Debug, Clone)]
pub struct DaemonCycleResources {
    program_config: CycleProgramConfig,
    pub markets: MarketsConfig,
    /// Operator network for Coinset IO and ledger fields (`mainnet` / `testnet11`).
    /// Sourced from program.yaml; prefer this over `program().network` at call sites.
    pub network: String,
    pub dexie: DexieClient,
    pub paths: DaemonCyclePaths,
    pub inventory_freshness: Arc<InventoryFreshnessCache>,
    /// Stable maker/inventory p2s for WS filters (computed once per daemon process when set).
    pub inventory_p2s: Arc<[String]>,
    pub ticker_index: CatTickerIndex,
}

impl DaemonCycleResources {
    #[must_use]
    pub fn program_path(&self) -> &std::path::Path {
        &self.paths.program_path
    }

    #[must_use]
    pub fn program(&self) -> &ManagerProgramConfig {
        self.program_config.program()
    }

    /// Signer for execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn signer_for_execution(&self) -> SignerResult<&SignerConfig> {
        self.program_config.signer_for_execution()
    }

    /// Signer-backed offer asset resolver for this cycle.
    ///
    /// # Errors
    ///
    /// Returns an error when signer config is unavailable for execution.
    pub fn asset_resolver(&self) -> SignerResult<crate::offer::OfferAssetResolver<'_>> {
        Ok(crate::offer::OfferAssetResolver::new(
            self.signer_for_execution()?,
            &self.ticker_index,
            &self.network,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn with_program_config(
        program_config: CycleProgramConfig,
        markets: MarketsConfig,
        network: String,
        dexie: DexieClient,
        paths: DaemonCyclePaths,
        inventory_freshness: Arc<InventoryFreshnessCache>,
        inventory_p2s: Arc<[String]>,
        ticker_index: CatTickerIndex,
    ) -> Self {
        Self {
            program_config,
            markets,
            network,
            dexie,
            paths,
            inventory_freshness,
            inventory_p2s,
            ticker_index,
        }
    }

    #[must_use]
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
    pub write_store: CycleWriteStore,
    pub allowed_key_ids: Vec<String>,
    pub xch_price_usd: Option<f64>,
    pub previous_xch_price_usd: Option<f64>,
    pub runtime_dry_run: bool,
    pub test_controls: super::run_once::DaemonCycleTestControls,
}

/// Per-market inputs for inventory → strategy → cancel → `coin_ops`.
#[derive(Debug, Clone)]
pub struct MarketCycleContext<'a> {
    pub resources: &'a DaemonCycleResources,
    pub dispatch: &'a MarketDispatchContext,
    pub plan: &'a CyclePlan,
    pub reconcile: &'a ReconcileMarketCycleResult,
}

impl MarketCycleContext<'_> {
    /// Owned gated operator bundle for one market row in this cycle.
    ///
    /// # Errors
    ///
    /// Returns an error when signer config is unavailable for execution.
    pub fn gated_market(&self, market: &MarketConfig) -> SignerResult<GatedOperatorMarket> {
        Ok(GatedOperatorMarket::assemble(
            self.resources.program().clone(),
            self.resources.signer_for_execution()?.clone(),
            market.clone(),
            self.resources.ticker_index.clone(),
            &self.resources.network,
        ))
    }
}

/// Load cycle resources.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_cycle_resources(request: &DaemonRunOnceRequest) -> SignerResult<DaemonCycleResources> {
    let loaded = load_daemon_cycle_config(
        &request.program_path,
        &request.markets_path,
        request.testnet_markets_path.as_deref(),
    )?;
    super::disabled_markets::log_disabled_markets_startup_once(&loaded.markets);
    let dexie = DexieClient::new(loaded.program_config.program().dexie_api_base.clone());
    let inventory_freshness = request.inventory_freshness.clone();
    let inventory_p2s = match &request.inventory_p2s {
        Some(p2s) => p2s.clone(),
        None => Arc::from(super::coinset_ws::stable_inventory_p2s_from_markets(
            &request.markets_path,
            request.testnet_markets_path.as_deref(),
        )?),
    };
    let ticker_index = operator_ticker_index_from_paths(
        &request.markets_path,
        request.testnet_markets_path.as_deref(),
        None,
    );
    Ok(DaemonCycleResources::with_program_config(
        loaded.program_config,
        loaded.markets,
        loaded.network,
        dexie,
        DaemonCyclePaths::new(
            request.program_path.clone(),
            request.markets_path.clone(),
            request.testnet_markets_path.clone(),
        ),
        inventory_freshness,
        inventory_p2s,
        ticker_index,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::config::empty_cat_ticker_index;

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
            InventoryFreshnessCache::new(),
            Arc::<[String]>::from(Vec::new()),
            empty_cat_ticker_index(),
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

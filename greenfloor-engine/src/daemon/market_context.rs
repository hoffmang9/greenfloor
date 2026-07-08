use std::sync::Arc;

use crate::adapters::DexieClient;
use crate::config::{
    load_daemon_cycle_config, operator_ticker_index_from_paths, CatTickerIndex, CycleProgramConfig,
    GatedOperatorMarket, ManagerProgramConfig, MarketConfig, MarketsConfig, SignerConfig,
};
use crate::error::SignerResult;
use crate::storage::CycleWriteStore;

use super::coinset_ws::{CoinsetProcessContext, InventoryP2Index};
use super::cycle_paths::DaemonCyclePaths;
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
    /// Process-scoped Coinset WS inventory filters + freshness.
    pub coinset: Arc<CoinsetProcessContext>,
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
        coinset: Arc<CoinsetProcessContext>,
        ticker_index: CatTickerIndex,
    ) -> Self {
        Self {
            program_config,
            markets,
            network,
            dexie,
            paths,
            coinset,
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
    let coinset = if request.coinset.inventory_p2s.p2s().is_empty() {
        let inventory_p2s = InventoryP2Index::from_markets(
            &request.markets_path,
            request.testnet_markets_path.as_deref(),
        )?;
        CoinsetProcessContext::new(
            inventory_p2s,
            Arc::clone(&request.coinset.inventory_freshness),
        )
    } else {
        Arc::clone(&request.coinset)
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
        coinset,
        ticker_index,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};
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
            CoinsetProcessContext::empty(),
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

    #[test]
    fn load_cycle_resources_rebuilds_empty_p2_index_preserving_freshness() {
        use crate::daemon::inventory_freshness::InventoryFreshnessCache;
        use crate::daemon::run_once::{
            DaemonCycleTestControls, DaemonDispatchState, DaemonRunOnceRequest,
        };
        use crate::test_support::minimal_program::{
            write_minimal_program_with_signer, MinimalProgramParams,
        };
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let program_path = dir.path().join("program.yaml");
        write_minimal_program_with_signer(
            &program_path,
            MinimalProgramParams {
                home_dir: dir.path(),
                ..Default::default()
            },
        );
        let markets_path = dir.path().join("markets.yaml");
        std::fs::write(
            &markets_path,
            r#"markets:
  - id: m1
    enabled: true
    base_asset: "xch"
    base_symbol: "XCH"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    ladders:
      sell:
        - size_base_units: 1
          target_count: 1
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
"#,
        )
        .expect("markets");
        let freshness = InventoryFreshnessCache::new();
        freshness.mark_fresh("m1", BTreeMap::from([(10, 1)]));
        let request = DaemonRunOnceRequest {
            program_path,
            markets_path,
            testnet_markets_path: None,
            state_db_override: None,
            coinset_base_url: "https://api.coinset.org".to_string(),
            state_dir: dir.path().to_path_buf(),
            poll_coinset_mempool: false,
            use_websocket_capture: false,
            allowed_key_ids: Vec::new(),
            dispatch_state: DaemonDispatchState::default(),
            test_controls: DaemonCycleTestControls::default(),
            coinset: CoinsetProcessContext::new(
                Arc::new(InventoryP2Index::default()),
                Arc::clone(&freshness),
            ),
        };
        let resources = load_cycle_resources(&request).expect("load");
        assert!(
            Arc::ptr_eq(
                &resources.coinset.inventory_freshness,
                &request.coinset.inventory_freshness
            ),
            "freshness Arc must be preserved across empty-index rebuild"
        );
        assert!(!resources
            .coinset
            .inventory_freshness
            .needs_refresh("m1", crate::daemon::INVENTORY_MAX_STALENESS));
    }
}

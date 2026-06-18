//! Test-only cycle context builders for daemon unit tests.

use std::path::Path;

use tempfile::TempDir;

use crate::adapters::DexieClient;
use crate::config::{
    CycleProgramConfig, ManagerProgramConfig, MarketsConfig, ProgramConfigBundle, SignerConfig,
};
use crate::cycle::StaleSweepProgress;

use super::cycle_paths::DaemonCyclePaths;
use super::market_context::{DaemonCycleResources, MarketCycleContext, MarketDispatchContext};
use super::reconcile_market_cycle::ReconcileMarketCycleResult;
use super::run_once::{CyclePlan, DaemonCycleTestControls, DaemonDispatchState};

pub struct TestCycleContextBundle {
    pub resources: DaemonCycleResources,
    pub dispatch: MarketDispatchContext,
    pub plan: CyclePlan,
    pub reconcile: ReconcileMarketCycleResult,
}

impl TestCycleContextBundle {
    pub fn cycle_context(&self) -> MarketCycleContext<'_> {
        MarketCycleContext {
            resources: &self.resources,
            dispatch: &self.dispatch,
            plan: &self.plan,
            reconcile: &self.reconcile,
        }
    }
}

pub fn test_cycle_context(
    dir: &TempDir,
    db_path: &Path,
    program: ManagerProgramConfig,
    signer: Option<SignerConfig>,
) -> TestCycleContextBundle {
    use std::collections::HashMap;

    let program_config = match signer {
        Some(signer) => {
            CycleProgramConfig::WithSigner(Box::new(ProgramConfigBundle { program, signer }))
        }
        None => CycleProgramConfig::WithoutSigner(Box::new(program)),
    };

    TestCycleContextBundle {
        resources: DaemonCycleResources::with_program_config(
            program_config,
            MarketsConfig { markets: vec![] },
            "mainnet".to_string(),
            DexieClient::new("https://api.dexie.space"),
            DaemonCyclePaths::new(
                dir.path().join("program.yaml"),
                dir.path().join("markets.yaml"),
                None,
            ),
            super::watchlist::CoinWatchlistCache::new(),
        ),
        dispatch: MarketDispatchContext {
            db_path: db_path.to_path_buf(),
            allowed_key_ids: Vec::new(),
            xch_price_usd: None,
            previous_xch_price_usd: None,
            runtime_dry_run: false,
            test_controls: DaemonCycleTestControls::default(),
        },
        plan: CyclePlan {
            enabled_market_ids: vec!["m1".to_string()],
            selected_market_ids: vec!["m1".to_string()],
            consumed_immediate_requeues: Vec::new(),
            dispatch_state: DaemonDispatchState::default(),
            stale_open_sweep: StaleSweepProgress {
                checked_offer_count: 0,
                requeue_market_ids: Vec::new(),
                hits: Vec::new(),
                truncated: false,
            },
            configured_market_slot_count: 1,
            runtime_dry_run: false,
            db_path: db_path.to_path_buf(),
            previous_xch_price_usd: None,
            dexie_base_url: "https://api.dexie.space".to_string(),
            splash_base_url: "http://example.test".to_string(),
            test_controls: DaemonCycleTestControls::default(),
        },
        reconcile: ReconcileMarketCycleResult {
            offers: Vec::new(),
            dexie_size_by_offer_id: HashMap::new(),
            dexie_fetch_error: None,
            metrics: Default::default(),
        },
    }
}

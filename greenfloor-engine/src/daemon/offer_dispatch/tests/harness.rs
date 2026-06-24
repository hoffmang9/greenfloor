use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::json;
use tempfile::TempDir;

use super::super::OfferDispatchOutput;
use super::fixtures::sample_program;
use crate::config::{load_program_bundle, ManagerProgramConfig, MarketConfig};
use crate::cycle::PlannedAction;
use crate::daemon::dispatch_test_controls::DaemonDispatchTestInjections;
use crate::daemon::test_support::test_cycle_context;
use crate::storage::CycleWriteStore;
use crate::test_support::market_config;
use crate::test_support::minimal_program::{
    write_minimal_program_with_signer, MinimalProgramParams,
};

pub(super) fn write_test_markets_file(path: &Path) {
    std::fs::write(
        path,
        r"
markets:
  - id: m1
    enabled: true
    base_asset: asset1
    base_symbol: AS1
    quote_asset: xch
    quote_asset_type: unstable
    receive_address: xch1test
    signer_key_id: key-1
    mode: sell_only
    pricing: {}
",
    )
    .expect("write markets");
}

pub(super) fn test_context_from_program_file(
    dir: &TempDir,
    db_path: &Path,
    write_store: CycleWriteStore,
    program_path: &Path,
    mut program: ManagerProgramConfig,
    with_signer: bool,
) -> crate::daemon::test_support::TestCycleContextBundle {
    let signer = if with_signer {
        let bundle = load_program_bundle(program_path).expect("program bundle");
        program.signer_kms_key_id = bundle.program.signer_kms_key_id;
        program.vault_launcher_id = bundle.program.vault_launcher_id;
        Some(bundle.signer)
    } else {
        None
    };
    test_cycle_context(dir, db_path, write_store, program, signer)
}

pub(super) fn sample_market() -> MarketConfig {
    let mut market = market_config::sample_market("xch1test");
    market.quote_asset_type = "stable".to_string();
    market
}

pub(super) fn sample_market_with_pricing() -> MarketConfig {
    MarketConfig {
        pricing: json!({
            "min_price_quote_per_base": 0.0031,
            "max_price_quote_per_base": 0.0038,
        }),
        ..sample_market()
    }
}

pub(super) fn sample_action() -> PlannedAction {
    PlannedAction {
        size: 1,
        repeat: 1,
        pair: "xch".to_string(),
        expiry_unit: "minutes".to_string(),
        expiry_value: 10,
        cancel_after_create: false,
        reason: "test".to_string(),
        target_spread_bps: None,
        side: "sell".to_string(),
    }
}

pub(super) struct ParallelDispatchHarness {
    pub(super) _dir: TempDir,
    pub(super) store: CycleWriteStore,
    pub(super) program_path: PathBuf,
    test_ctx: crate::daemon::test_support::TestCycleContextBundle,
}

impl ParallelDispatchHarness {
    pub(super) fn new(parallelism_enabled: bool, dry_run: bool, with_signer: bool) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        let store = CycleWriteStore::open(&db_path).expect("open");
        let program_path = dir.path().join("program.yaml");
        write_minimal_program_with_signer(
            &program_path,
            MinimalProgramParams {
                home_dir: dir.path(),
                ..Default::default()
            },
        );
        let markets_path = dir.path().join("markets.yaml");
        write_test_markets_file(&markets_path);
        let test_ctx = test_context_from_program_file(
            &dir,
            &db_path,
            store.clone(),
            &program_path,
            sample_program(parallelism_enabled, dry_run),
            with_signer,
        );
        Self {
            _dir: dir,
            store,
            program_path,
            test_ctx,
        }
    }

    pub(super) fn set_offer_dispatch(&mut self, injections: DaemonDispatchTestInjections) {
        self.test_ctx.dispatch.test_controls.offer_dispatch = injections;
    }

    pub(super) async fn execute(
        &self,
        market: &MarketConfig,
        actions: &[PlannedAction],
    ) -> crate::error::SignerResult<OfferDispatchOutput> {
        use super::super::execute_strategy_actions;

        execute_strategy_actions(&self.test_ctx.cycle_context(), market, actions).await
    }

    pub(super) fn managed_post_context(&self) -> super::super::managed_post::ManagedPostContext {
        super::super::managed_post::ManagedPostContext::from_market_cycle(
            &self.test_ctx.cycle_context(),
        )
    }
}

pub(super) fn assert_persist_flush_does_not_reopen_cycle_db(
    post_ctx: &super::super::managed_post::ManagedPostContext,
) {
    use crate::storage::{reset_sqlite_open_calls_for_test, sqlite_open_calls_for_test};

    use super::super::managed_post::flush_managed_post_persist_for_test;

    reset_sqlite_open_calls_for_test();
    flush_managed_post_persist_for_test(post_ctx).expect("flush");
    assert_eq!(sqlite_open_calls_for_test(), 0);
}

pub(super) async fn generous_spendable_profiles(
    program_path: &Path,
    market: &MarketConfig,
) -> BTreeMap<String, crate::cycle::SpendableAssetProfile> {
    use crate::cycle::SpendableAssetProfile;
    use crate::daemon::offer_dispatch::reservation_ctx::{
        parallel_reservation_asset_ids, parallel_reservation_context,
    };

    use crate::config::{empty_cat_ticker_index, load_program_bundle};
    use crate::offer::OfferAssetResolver;

    let bundle = load_program_bundle(program_path).expect("program bundle");
    let empty_index = empty_cat_ticker_index();
    let resolver = OfferAssetResolver::new(&bundle.signer, &empty_index, "mainnet");
    let reservation_ctx = parallel_reservation_context(&resolver, market, 0)
        .await
        .expect("reservation ctx");
    let mut spendable_profiles = BTreeMap::new();
    for asset_id in parallel_reservation_asset_ids(&reservation_ctx) {
        spendable_profiles.insert(
            asset_id,
            SpendableAssetProfile {
                total: 999_999_999,
                max_single: 999_999_999,
                max_single_known: true,
            },
        );
    }
    spendable_profiles
}

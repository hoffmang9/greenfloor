//! Shared harness for `run_coin_ops_phase` integration tests.
#![allow(clippy::missing_panics_doc)] // test harness: panics on fixture setup failure

use std::collections::BTreeMap;
use std::path::PathBuf;

use tempfile::TempDir;

use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::daemon::test_support::test_cycle_context;
use crate::storage::{state_db_path_for_home, CoinOpLedgerEntry, SqliteStore};
use crate::test_support::ladder::market_with_sell_ladder;
use crate::test_support::market_config::sample_market;
use crate::test_support::minimal_program::{
    write_minimal_program_with_signer, MinimalProgramParams,
};

use super::run_coin_ops_phase;

pub struct CoinOpsPhaseHarness {
    pub store: SqliteStore,
    _dir: TempDir,
    ctx: crate::daemon::test_support::TestCycleContextBundle,
}

impl CoinOpsPhaseHarness {
    pub fn open(
        configure_program: impl FnOnce(&mut ManagerProgramConfig),
        ledger_seed: Option<CoinOpLedgerEntry<'static>>,
    ) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let program_path: PathBuf = dir.path().join("program.yaml");
        write_minimal_program_with_signer(
            &program_path,
            MinimalProgramParams {
                home_dir: dir.path(),
                ..Default::default()
            },
        );
        let mut bundle = crate::config::load_program_bundle(&program_path).expect("bundle");
        bundle.program.coin_ops_max_operations_per_run = 20;
        configure_program(&mut bundle.program);
        let db_path = state_db_path_for_home(dir.path());
        let store = SqliteStore::open(&db_path).expect("open");
        if let Some(entry) = ledger_seed {
            store.add_coin_op_ledger_entry(&entry).expect("seed ledger");
        }
        let ctx = test_cycle_context(&dir, &db_path, bundle.program.clone(), Some(bundle.signer));
        Self {
            store,
            _dir: dir,
            ctx,
        }
    }

    pub async fn run_with_market(&self, market: &MarketConfig, wallet_counts: &BTreeMap<i64, i64>) {
        run_coin_ops_phase(
            &self.store,
            &self.ctx.cycle_context(),
            market,
            &[],
            wallet_counts,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .await
        .expect("coin ops phase");
    }

    pub async fn run_with_sell_ladder(&self, wallet_counts: &BTreeMap<i64, i64>) {
        let market = market_with_sell_ladder("xch1test", 10, 5);
        self.run_with_market(&market, wallet_counts).await;
    }

    pub async fn run_empty_sell_ladder(&self) {
        let market = sample_market("xch1test");
        self.run_with_market(&market, &BTreeMap::new()).await;
    }
}

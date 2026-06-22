use std::collections::BTreeMap;

use super::ladder::build_valid_sell_ladder;
use super::ladder::classify_sell_ladder_entries;
use super::ladder::record_sub_minimum_sell_ladder_skips;
use super::{
    apply_overflow_plan_skips, record_coin_ops_phase_audit, skipped_coin_ops_result,
    CoinOpsPlanningResult,
};
use crate::coin_ops::{CoinOpKind, CoinOpPlan};
use crate::config::{LadderEntry, ManagerProgramConfig};
use crate::daemon::coin_ops_execution::CoinOpExecutionResult;
use crate::operator_log::{
    COIN_OPS_NO_PLANS, COIN_OPS_PARTIAL_OR_SKIPPED_FEE_BUDGET, COIN_OPS_PLAN,
    COIN_OPS_SKIPPED_FEE_BUDGET, COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT,
};
use crate::storage::{state_db_path_for_home, CoinOpLedgerEntry, SqliteStore};
use crate::test_support::ladder::market_with_sell_ladder;
use crate::test_support::market_config::sample_market;

mod fee_budget_harness {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use crate::config::{ManagerProgramConfig, MarketConfig};
    use crate::daemon::coin_ops_phase::run_coin_ops_phase;
    use crate::daemon::test_support::test_cycle_context;
    use crate::storage::{state_db_path_for_home, CoinOpLedgerEntry, SqliteStore};
    use crate::test_support::ladder::market_with_sell_ladder;
    use crate::test_support::market_config::sample_market;
    use crate::test_support::minimal_program::{
        write_minimal_program_with_signer, MinimalProgramParams,
    };

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
            let ctx =
                test_cycle_context(&dir, &db_path, bundle.program.clone(), Some(bundle.signer));
            Self {
                store,
                _dir: dir,
                ctx,
            }
        }

        pub async fn run_with_market(
            &self,
            market: &MarketConfig,
            wallet_counts: &BTreeMap<i64, i64>,
        ) {
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
}

use fee_budget_harness::CoinOpsPhaseHarness;

#[test]
fn classify_sell_ladder_entries_filters_zero_size_rows() {
    let ladder = vec![
        LadderEntry {
            size_base_units: 0,
            target_count: 1,
            split_buffer_count: 0,
            combine_when_excess_factor: 2.0,
        },
        LadderEntry {
            size_base_units: 10,
            target_count: 2,
            split_buffer_count: 0,
            combine_when_excess_factor: 2.0,
        },
    ];
    let (valid, invalid) = classify_sell_ladder_entries("xch", 1_000_000_000, &ladder);
    assert_eq!(valid.len(), 1);
    assert_eq!(valid[0].size_base_units, 10);
    assert!(invalid.is_empty());
}

#[test]
fn classify_sell_ladder_entries_rejects_sub_minimum_cat_targets() {
    let cat_id = "b".repeat(64);
    let ladder = vec![LadderEntry {
        size_base_units: 1,
        target_count: 1,
        split_buffer_count: 0,
        combine_when_excess_factor: 2.0,
    }];
    let (valid, invalid) = classify_sell_ladder_entries(&cat_id, 500, &ladder);
    assert!(valid.is_empty());
    assert_eq!(invalid.len(), 1);
    assert_eq!(invalid[0]["target_amount_mojos"], 500);
}

#[test]
fn sub_minimum_sell_ladder_skips_emit_audit_event() {
    let cat_id = "b".repeat(64);
    let ladder = vec![LadderEntry {
        size_base_units: 1,
        target_count: 1,
        split_buffer_count: 0,
        combine_when_excess_factor: 2.0,
    }];
    let (_, invalid) = classify_sell_ladder_entries(&cat_id, 500, &ladder);
    assert_eq!(invalid.len(), 1);

    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let store = SqliteStore::open(&state_db_path_for_home(&home)).expect("open");
    let mut market = sample_market("xch1test");
    market.base_asset = cat_id;

    record_sub_minimum_sell_ladder_skips(&store, &market, &invalid).expect("audit");

    let events = store
        .list_recent_audit_events(
            Some(&[COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT]),
            Some("m1"),
            5,
        )
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["invalid_bucket_count"].as_u64(), Some(1));
}

#[test]
fn build_valid_sell_ladder_accepts_minimum_cat_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let store = SqliteStore::open(&state_db_path_for_home(&home)).expect("open");
    let mut market = sample_market("xch1test");
    market.base_asset = "b".repeat(64);
    let ladder = vec![LadderEntry {
        size_base_units: 1,
        target_count: 1,
        split_buffer_count: 0,
        combine_when_excess_factor: 2.0,
    }];

    let valid = build_valid_sell_ladder(&store, &market, &ladder).expect("ladder");
    assert_eq!(valid.len(), 1);
    let events = store
        .list_recent_audit_events(
            Some(&[COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT]),
            Some("m1"),
            5,
        )
        .expect("events");
    assert!(events.is_empty());
}

#[test]
fn apply_overflow_plan_skips_marks_fee_budget_guard() {
    let mut execution = CoinOpExecutionResult {
        dry_run: false,
        planned_count: 0,
        executed_count: 0,
        status: "ok".to_string(),
        items: Vec::new(),
        signer_selection: serde_json::json!({}),
    };
    let overflow = vec![CoinOpPlan {
        op_type: CoinOpKind::Split,
        size_base_units: 10,
        op_count: 2,
        reason: "deficit".to_string(),
    }];

    apply_overflow_plan_skips(&mut execution, &overflow);

    assert_eq!(execution.items.len(), 1);
    assert_eq!(execution.items[0].status, "skipped");
    assert_eq!(execution.items[0].reason, "fee_budget_guard");
}

#[test]
fn skipped_coin_ops_result_marks_all_plans_skipped() {
    let program = ManagerProgramConfig {
        runtime_dry_run: true,
        network: "mainnet".to_string(),
        ..Default::default()
    };
    let market = sample_market("xch1test");
    let plans = vec![CoinOpPlan {
        op_type: CoinOpKind::Combine,
        size_base_units: 5,
        op_count: 1,
        reason: "test".to_string(),
    }];

    let result = skipped_coin_ops_result(&program, &market, &plans, "signer_unavailable");

    assert_eq!(result.status, "skipped");
    assert_eq!(result.planned_count, 1);
    assert_eq!(result.executed_count, 0);
    assert!(result
        .items
        .iter()
        .all(|item| item.reason == "signer_unavailable"));
    assert_eq!(
        result.signer_selection["key_id"].as_str(),
        Some(market.signer_key_id.as_str())
    );
}

#[tokio::test]
async fn run_coin_ops_phase_noops_on_empty_sell_ladder() {
    let harness = CoinOpsPhaseHarness::open(|_| {}, None);
    harness.run_empty_sell_ladder().await;

    let events = harness
        .store
        .list_recent_audit_events(Some(&[COIN_OPS_NO_PLANS]), Some("m1"), 5)
        .expect("events");
    assert!(events.iter().any(|event| {
        event.payload.get("reason").and_then(|value| value.as_str()) == Some("empty_sell_ladder")
    }));
}

#[tokio::test]
async fn run_coin_ops_phase_skips_execution_when_daily_fee_budget_exhausted() {
    let harness = CoinOpsPhaseHarness::open(
        |program| {
            program.coin_ops_max_daily_fee_budget_mojos = 100;
            program.coin_ops_split_fee_mojos = 10;
            program.coin_ops_combine_fee_mojos = 0;
        },
        Some(CoinOpLedgerEntry {
            market_id: "m1",
            op_type: "split",
            op_count: 1,
            fee_mojos: 95,
            status: "executed",
            reason: "seed_spent_today",
            operation_id: None,
        }),
    );
    let wallet_counts = BTreeMap::from([(10_i64, 0_i64)]);
    harness.run_with_sell_ladder(&wallet_counts).await;

    let events = harness
        .store
        .list_recent_audit_events(
            Some(&[COIN_OPS_PLAN, COIN_OPS_SKIPPED_FEE_BUDGET]),
            Some("m1"),
            10,
        )
        .expect("events");
    assert!(events.iter().any(|event| event.event_type == COIN_OPS_PLAN));
    assert!(events
        .iter()
        .any(|event| event.event_type == COIN_OPS_SKIPPED_FEE_BUDGET));
}

#[tokio::test]
async fn run_coin_ops_phase_records_partial_fee_budget_overflow() {
    let harness = CoinOpsPhaseHarness::open(
        |program| {
            program.coin_ops_max_daily_fee_budget_mojos = 55;
            program.coin_ops_split_fee_mojos = 10;
            program.coin_ops_combine_fee_mojos = 0;
        },
        None,
    );
    let wallet_counts = BTreeMap::from([(10_i64, 0_i64)]);
    harness.run_with_sell_ladder(&wallet_counts).await;

    let events = harness
        .store
        .list_recent_audit_events(
            Some(&[
                COIN_OPS_PLAN,
                COIN_OPS_PARTIAL_OR_SKIPPED_FEE_BUDGET,
                COIN_OPS_SKIPPED_FEE_BUDGET,
            ]),
            Some("m1"),
            10,
        )
        .expect("events");
    assert!(events.iter().any(|event| event.event_type == COIN_OPS_PLAN));
    assert!(events
        .iter()
        .any(|event| event.event_type == COIN_OPS_PARTIAL_OR_SKIPPED_FEE_BUDGET));
}

#[test]
fn record_coin_ops_phase_audit_logs_skipped_fee_budget_for_empty_executable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let store = SqliteStore::open(&state_db_path_for_home(&home)).expect("open");
    let market = market_with_sell_ladder("xch1test", 10, 5);
    let program = ManagerProgramConfig {
        coin_ops_max_daily_fee_budget_mojos: 100,
        coin_ops_split_fee_mojos: 10,
        ..Default::default()
    };
    let planning = CoinOpsPlanningResult {
        plans: vec![CoinOpPlan {
            op_type: CoinOpKind::Split,
            size_base_units: 10,
            op_count: 2,
            reason: "deficit".to_string(),
        }],
        projected_fee: 20,
        spent_today: 95,
        executable_plans: Vec::new(),
        overflow_plans: vec![CoinOpPlan {
            op_type: CoinOpKind::Split,
            size_base_units: 10,
            op_count: 2,
            reason: "deficit".to_string(),
        }],
    };
    let execution = CoinOpExecutionResult {
        dry_run: false,
        planned_count: 1,
        executed_count: 0,
        status: "skipped_fee_budget".to_string(),
        items: Vec::new(),
        signer_selection: serde_json::json!({}),
    };

    record_coin_ops_phase_audit(&store, &market, &program, &planning, &execution).expect("audit");

    let events = store
        .list_recent_audit_events(
            Some(&[COIN_OPS_PLAN, COIN_OPS_SKIPPED_FEE_BUDGET]),
            Some("m1"),
            5,
        )
        .expect("events");
    assert!(events.iter().any(|event| event.event_type == COIN_OPS_PLAN));
    assert!(events
        .iter()
        .any(|event| event.event_type == COIN_OPS_SKIPPED_FEE_BUDGET));
}

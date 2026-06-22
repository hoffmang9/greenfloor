use std::collections::BTreeMap;

use super::ladder::build_valid_sell_ladder;
use super::ladder::classify_sell_ladder_entries;
use super::ladder::record_sub_minimum_sell_ladder_skips;
use super::{apply_overflow_plan_skips, skipped_coin_ops_result};
use crate::coin_ops::{CoinOpKind, CoinOpPlan};
use crate::config::{LadderEntry, ManagerProgramConfig};
use crate::daemon::coin_ops_execution::CoinOpExecutionResult;
use crate::daemon::CoinOpsPhaseHarness;
use crate::operator_log::{
    COIN_OPS_NO_PLANS, COIN_OPS_PARTIAL_OR_SKIPPED_FEE_BUDGET, COIN_OPS_PLAN,
    COIN_OPS_SKIPPED_FEE_BUDGET, COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT,
};
use crate::storage::{state_db_path_for_home, CoinOpLedgerEntry, SqliteStore};
use crate::test_support::ladder::market_with_side_ladder;
use crate::test_support::market_config::sample_market;

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

#[tokio::test]
async fn run_coin_ops_phase_runs_with_minimum_cat_sell_ladder() {
    let cat_id = "b".repeat(64);
    let mut market = market_with_side_ladder("xch1test", "sell", 1, 1);
    market.base_asset = cat_id;
    let harness = CoinOpsPhaseHarness::open(|_| {}, None);
    harness
        .run_with_market(&market, &BTreeMap::from([(1_i64, 0_i64)]))
        .await;

    let events = harness
        .store
        .list_recent_audit_events(
            Some(&[COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT, COIN_OPS_PLAN]),
            Some("m1"),
            10,
        )
        .expect("events");
    assert!(events
        .iter()
        .all(|event| event.event_type != COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT));
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

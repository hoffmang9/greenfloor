use std::collections::HashSet;

use crate::coin_ops::{CoinOpKind, CoinOpPlan};
use crate::config::{load_program_bundle, MarketConfig};
use crate::daemon::coin_ops_execution::{execute_managed_coin_op_plans, CoinOpExecutionResult};
use crate::test_support::minimal_program::{
    write_minimal_program_with_signer, MinimalProgramParams,
};

fn sample_market(receive_address: &str) -> MarketConfig {
    MarketConfig {
        market_id: "m1".to_string(),
        enabled: true,
        base_asset: "xch".to_string(),
        base_symbol: "XCH".to_string(),
        quote_asset: "xch".to_string(),
        quote_asset_type: "unstable".to_string(),
        receive_address: receive_address.to_string(),
        signer_key_id: "key-main-1".to_string(),
        mode: "sell_only".to_string(),
        pricing: serde_json::json!({}),
        cancel_move_threshold_bps: None,
        ladders: std::collections::HashMap::default(),
    }
}

fn sample_plan(op_type: CoinOpKind) -> CoinOpPlan {
    CoinOpPlan {
        op_type,
        size_base_units: 10,
        op_count: 2,
        reason: "test".to_string(),
    }
}

fn assert_skipped_all(result: &CoinOpExecutionResult, reason: &str) {
    assert_eq!(result.executed_count, 0);
    assert_eq!(result.status, "skipped");
    assert!(result.items.iter().all(|item| item.status == "skipped"));
    assert!(result.items.iter().all(|item| item.reason.contains(reason)));
}

#[tokio::test]
async fn execute_managed_coin_op_plans_skips_when_receive_address_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program_path = dir.path().join("program.yaml");
    write_minimal_program_with_signer(
        &program_path,
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    let bundle = load_program_bundle(&program_path).expect("bundle");
    let market = sample_market("");
    let plans = vec![
        sample_plan(CoinOpKind::Split),
        sample_plan(CoinOpKind::Combine),
    ];

    let result = execute_managed_coin_op_plans(
        &bundle.program,
        &bundle.signer,
        &market,
        &plans,
        &HashSet::<String>::default(),
    )
    .await;

    assert_skipped_all(&result, "signer_coin_ops_missing_receive_address");
    assert_eq!(result.planned_count, 2);
}

#[tokio::test]
async fn execute_managed_coin_op_plans_dry_run_plans_without_execution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program_path = dir.path().join("program.yaml");
    write_minimal_program_with_signer(
        &program_path,
        MinimalProgramParams {
            home_dir: dir.path(),
            dry_run: true,
            ..Default::default()
        },
    );
    let mut bundle = load_program_bundle(&program_path).expect("bundle");
    bundle.program.runtime_dry_run = true;
    let market = sample_market("xch1test");
    let plans = vec![sample_plan(CoinOpKind::Split)];

    let result = execute_managed_coin_op_plans(
        &bundle.program,
        &bundle.signer,
        &market,
        &plans,
        &HashSet::<String>::default(),
    )
    .await;

    assert_eq!(result.executed_count, 0);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].status, "planned");
    assert_eq!(result.items[0].reason, "dry_run:signer");
}

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use crate::coin_ops::{CoinOpKind, CoinOpPlan};
use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::daemon::coin_ops_execution::{
    execute_managed_coin_op_plans, CoinOpExecutionResult,
};

fn sample_program() -> ManagerProgramConfig {
    ManagerProgramConfig {
        network: "mainnet".to_string(),
        home_dir: std::path::PathBuf::from("/tmp/gf"),
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
        runtime_market_slot_count: 0,
        runtime_offer_parallelism_enabled: false,
        runtime_offer_parallelism_max_workers: 4,
        runtime_dry_run: false,
        runtime_loop_interval_seconds: 30,
        tx_block_trigger_mode: "websocket".to_string(),
        tx_block_websocket_url: String::new(),
        tx_block_websocket_reconnect_interval_seconds: 1,
        tx_block_fallback_poll_interval_seconds: 1,
    }
}

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
        ladders: std::collections::HashMap::new(),
    }
}

fn write_signer_program(path: &Path) {
    let mut file = std::fs::File::create(path).expect("create program");
    write!(
        file,
        r#"
app:
  network: mainnet
  home_dir: /tmp/gf
runtime:
  offer_bootstrap_wait_timeout_seconds: 120
venues:
  dexie:
    api_base: https://api.dexie.space
  splash:
    api_base: http://localhost:4000
  offer_publish:
    provider: dexie
signer:
  kms_key_id: arn:aws:kms:us-west-2:123:key/demo
vault:
  launcher_id: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  custody_threshold: 1
  recovery_threshold: 1
  recovery_clawback_timelock: 3600
  custody_keys:
    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"
      curve: SECP256R1
  recovery_keys:
    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"
      curve: BLS12_381
"#
    )
    .expect("write program");
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
    assert!(result
        .items
        .iter()
        .all(|item| item.reason.contains(reason)));
}

#[tokio::test]
async fn execute_managed_coin_op_plans_skips_when_receive_address_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program_path = dir.path().join("program.yaml");
    write_signer_program(&program_path);
    let market = sample_market("");
    let program = sample_program();
    let plans = vec![sample_plan(CoinOpKind::Split), sample_plan(CoinOpKind::Combine)];

    let result = execute_managed_coin_op_plans(
        &program_path,
        &market,
        &program,
        &plans,
        &HashSet::new(),
    )
    .await;

    assert_skipped_all(&result, "signer_coin_ops_missing_receive_address");
    assert_eq!(result.planned_count, 2);
}

#[tokio::test]
async fn execute_managed_coin_op_plans_dry_run_plans_without_execution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program_path = dir.path().join("program.yaml");
    write_signer_program(&program_path);
    let market = sample_market("xch1test");
    let mut program = sample_program();
    program.runtime_dry_run = true;
    let plans = vec![sample_plan(CoinOpKind::Split)];

    let result = execute_managed_coin_op_plans(
        &program_path,
        &market,
        &program,
        &plans,
        &HashSet::new(),
    )
    .await;

    assert_eq!(result.executed_count, 0);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].status, "planned");
    assert_eq!(result.items[0].reason, "dry_run:signer");
}

use super::coinset_context::{load_gated_execution_signer, resolve_combine_coinset_context};
use super::combine_test_support::{
    dust_plan_from_scan_without_lineage, sample_job, test_coinset_context,
};
use super::report::{preview_job_report, vault_signer_ready};
use super::sim_harness::sim_dust_scan_result;
use super::{run_combine_market_cat_dust, CombineExecutionFlags, CombineMarketCatDustRequest};
use crate::coinset::CoinSpentVerifyConfig;
use crate::config::{load_program_config, parse_program_config, read_program_yaml};
use crate::manager_cli::test_support::{
    pop_json, write_combine_test_configs, ManagerContextBuilder,
};
use crate::minimal_program_template::{materialize_minimal_program_text, MinimalProgramParams};
use chia_sdk_coinset::ChiaRpcClient;
use serde_json::json;

#[tokio::test]
async fn preview_job_report_plans_two_coin_batch_from_simulator_scan() {
    let (scan, harness) = sim_dust_scan_result(&[400, 300]);
    let job = sample_job(
        scan.coins
            .first()
            .and_then(|row| row.cat_asset_id.as_deref())
            .expect("asset id"),
    );
    let dir = tempfile::tempdir().expect("tempdir");
    write_combine_test_configs(dir.path(), &job.cat_asset_id, true);
    let program = load_program_config(&dir.path().join("program.yaml")).expect("program");
    let readiness = vault_signer_ready(&program, &job.signer_key_id);
    assert!(readiness.can_combine);

    let plan = dust_plan_from_scan_without_lineage(&scan, &harness, 1000, 2);
    assert_eq!(plan.scan_dust_count, 2);
    assert_eq!(plan.batches.combinable_batches.len(), 1);
    assert!(plan.batches.uncombinable.is_empty());

    let coinset = test_coinset_context();
    let report = preview_job_report(&job, &scan, &coinset, &plan, readiness);
    assert_eq!(report.get("status"), Some(&json!("ok")));
    assert_eq!(report.get("combine_batches_planned"), Some(&json!(1)));
    assert_eq!(report.get("uncombinable_dust_count"), Some(&json!(0)));
    assert_eq!(
        report.get("coinset_base_url"),
        Some(&json!("https://api.coinset.org"))
    );

    let batches = report
        .get("batches")
        .and_then(|value| value.as_array())
        .expect("batches");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].get("status"), Some(&json!("preview")));
    assert_eq!(batches[0].get("would_combine"), Some(&json!(true)));
    assert_eq!(
        batches[0]
            .get("coin_ids")
            .and_then(|value| value.as_array())
            .map(std::vec::Vec::len),
        Some(2)
    );
}

#[tokio::test]
async fn preview_job_report_marks_single_dust_coin_as_orphan() {
    let (scan, harness) = sim_dust_scan_result(&[500]);
    let job = sample_job(
        scan.coins
            .first()
            .and_then(|row| row.cat_asset_id.as_deref())
            .expect("asset id"),
    );
    let dir = tempfile::tempdir().expect("tempdir");
    write_combine_test_configs(dir.path(), &job.cat_asset_id, true);
    let program = load_program_config(&dir.path().join("program.yaml")).expect("program");
    let readiness = vault_signer_ready(&program, &job.signer_key_id);
    let plan = dust_plan_from_scan_without_lineage(&scan, &harness, 1000, 2);
    assert_eq!(plan.scan_dust_count, 1);
    assert_eq!(plan.batches.combinable_batches.len(), 0);
    assert_eq!(plan.batches.uncombinable.len(), 1);

    let report = preview_job_report(&job, &scan, &test_coinset_context(), &plan, readiness);
    let batches = report
        .get("batches")
        .and_then(|value| value.as_array())
        .expect("batches");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].get("status"), Some(&json!("orphan")));
}

#[tokio::test]
async fn preview_job_report_without_signer_backend() {
    let (scan, harness) = sim_dust_scan_result(&[500]);
    let job = sample_job(
        scan.coins
            .first()
            .and_then(|row| row.cat_asset_id.as_deref())
            .expect("asset id"),
    );
    let dir = tempfile::tempdir().expect("tempdir");
    write_combine_test_configs(dir.path(), &job.cat_asset_id, false);
    let program = load_program_config(&dir.path().join("program.yaml")).expect("program");
    let readiness = vault_signer_ready(&program, &job.signer_key_id);
    assert!(!readiness.can_combine);
    assert_eq!(readiness.note, Some("signer_not_configured"));

    let plan = dust_plan_from_scan_without_lineage(&scan, &harness, 1000, 2);
    let report = preview_job_report(&job, &scan, &test_coinset_context(), &plan, readiness);
    assert_eq!(report.get("signer_config_ok"), Some(&json!(false)));
    assert_eq!(
        report.get("signer_config_note"),
        Some(&json!("signer_not_configured"))
    );
}

#[tokio::test]
async fn run_combine_emits_json_when_launcher_id_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cat_hex = "f".repeat(64);
    write_combine_test_configs(dir.path(), &cat_hex, false);

    let harness = ManagerContextBuilder::new(
        dir.path().join("program.yaml"),
        dir.path().join("markets.yaml"),
    )
    .cats_config(dir.path().join("cats.yaml"))
    .build_capturing();

    let exit = run_combine_market_cat_dust(CombineMarketCatDustRequest {
        mgr: &harness.ctx,
        network: Some("mainnet"),
        coinset_base_url: None,
        launcher_id: None,
        launcher_id_file: None,
        dust_threshold_mojos: 1000,
        max_input_coins: 2,
        max_nonce: 0,
        cat_asset_id: None,
        verify: CoinSpentVerifyConfig::default(),
        execution: CombineExecutionFlags::from_flags(false, true),
    })
    .await
    .expect("command");

    assert_eq!(exit, 1);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("status"), Some(&json!("error")));
    assert_eq!(
        payload.get("reason"),
        Some(&json!("launcher_id_resolution_failed"))
    );
}

#[tokio::test]
async fn run_combine_preview_does_not_require_signer_bundle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cat_hex = "f".repeat(64);
    write_combine_test_configs(dir.path(), &cat_hex, false);

    let harness = ManagerContextBuilder::new(
        dir.path().join("program.yaml"),
        dir.path().join("markets.yaml"),
    )
    .cats_config(dir.path().join("cats.yaml"))
    .build_capturing();

    let _exit = run_combine_market_cat_dust(CombineMarketCatDustRequest {
        mgr: &harness.ctx,
        network: Some("mainnet"),
        coinset_base_url: Some("http://127.0.0.1:1"),
        launcher_id: Some(&"aa".repeat(32)),
        launcher_id_file: None,
        dust_threshold_mojos: 1000,
        max_input_coins: 2,
        max_nonce: 0,
        cat_asset_id: None,
        verify: CoinSpentVerifyConfig::default(),
        execution: CombineExecutionFlags::from_flags(false, true),
    })
    .await
    .expect("command");

    let payload = pop_json(&harness.captured);
    assert_ne!(
        payload.get("reason"),
        Some(&json!("signer_not_configured")),
        "preview must not require signer bundle"
    );
    assert!(payload.get("jobs").is_some());
}

#[test]
fn load_gated_execution_signer_returns_program_signer_without_mutation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cat_hex = "f".repeat(64);
    write_combine_test_configs(dir.path(), &cat_hex, true);
    let program_path = dir.path().join("program.yaml");
    let raw = read_program_yaml(&program_path).expect("read program");
    let program = parse_program_config(&raw).expect("parse program");
    let coinset_ctx = resolve_combine_coinset_context(
        Some("testnet"),
        Some("https://coinset.custom/"),
        &program.network,
        "https://api.coinset.org",
    );
    let signer = load_gated_execution_signer(&raw, program.clone()).expect("execution signer");
    assert_eq!(signer.network, program.network);
    assert_ne!(signer.network, coinset_ctx.network);
    let client = coinset_ctx.client().expect("coinset client");
    assert_eq!(client.base_url(), "https://coinset.custom");
}

#[tokio::test]
async fn run_combine_live_emits_json_when_signer_bundle_invalid() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cat_hex = "f".repeat(64);
    write_combine_test_configs(dir.path(), &cat_hex, true);
    let program_path = dir.path().join("program.yaml");
    let program_text = std::fs::read_to_string(&program_path)
        .expect("read program")
        .replace(&"aa".repeat(32), "not-a-valid-launcher-id");
    std::fs::write(&program_path, program_text).expect("write invalid vault launcher");

    let harness = ManagerContextBuilder::new(program_path, dir.path().join("markets.yaml"))
        .cats_config(dir.path().join("cats.yaml"))
        .build_capturing();

    let exit = run_combine_market_cat_dust(CombineMarketCatDustRequest {
        mgr: &harness.ctx,
        network: Some("mainnet"),
        coinset_base_url: None,
        launcher_id: None,
        launcher_id_file: None,
        dust_threshold_mojos: 1000,
        max_input_coins: 2,
        max_nonce: 0,
        cat_asset_id: None,
        verify: CoinSpentVerifyConfig::default(),
        execution: CombineExecutionFlags::from_flags(false, false),
    })
    .await
    .expect("command");

    assert_eq!(exit, 1);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("status"), Some(&json!("error")));
    assert_eq!(payload.get("reason"), Some(&json!("signer_load_failed")));
    assert!(payload
        .get("detail")
        .and_then(serde_json::Value::as_str)
        .is_some());
}

#[tokio::test]
async fn run_combine_dry_run_reports_no_enabled_cat_markets() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cat_hex = "f".repeat(64);
    std::fs::write(
        dir.path().join("program.yaml"),
        materialize_minimal_program_text(MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        }),
    )
    .expect("write program");
    std::fs::write(
        dir.path().join("markets.yaml"),
        r#"markets:
  - id: m1
    enabled: true
    base_asset: "not_in_cats_catalog"
    base_symbol: "NOPE"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
"#,
    )
    .expect("write markets");
    std::fs::write(
        dir.path().join("cats.yaml"),
        format!("cats:\n  - base_symbol: DUST\n    asset_id: \"{cat_hex}\"\n"),
    )
    .expect("write cats");

    let harness = ManagerContextBuilder::new(
        dir.path().join("program.yaml"),
        dir.path().join("markets.yaml"),
    )
    .cats_config(dir.path().join("cats.yaml"))
    .build_capturing();

    let exit = run_combine_market_cat_dust(CombineMarketCatDustRequest {
        mgr: &harness.ctx,
        network: Some("mainnet"),
        coinset_base_url: None,
        launcher_id: None,
        launcher_id_file: None,
        dust_threshold_mojos: 1000,
        max_input_coins: 2,
        max_nonce: 0,
        cat_asset_id: None,
        verify: CoinSpentVerifyConfig::default(),
        execution: CombineExecutionFlags::from_flags(false, true),
    })
    .await
    .expect("command");

    assert_eq!(exit, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("status"), Some(&json!("ok")));
    assert_eq!(
        payload.get("message"),
        Some(&json!("no_enabled_cat_markets"))
    );
    assert_eq!(payload.get("jobs"), Some(&json!([])));
}

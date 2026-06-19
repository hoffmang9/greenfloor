use super::report::test_support::sim_dust_scan_result;
use super::report::{
    finalize_job_report, plan_dust_for_scan, preview_job_report, vault_signer_ready, JobExecution,
};
use super::{run_combine_market_cat_dust, CombineExecution, CombineMarketCatDustRequest};
use crate::config::load_program_config;
use crate::manager_cli::combine_market_cat_dust::jobs::CatDustJob;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::json::ManagerOutput;
use crate::minimal_program_template::{materialize_minimal_program_text, MinimalProgramParams};
use crate::test_support::simulator::harness::fetch_cat_from_sim_by_id;
use crate::vault::members::hex_to_bytes32;
use serde_json::json;

const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

const MINIMAL_PROGRAM_SIGNER_APPEND: &str =
    include_str!("../../../../tests/fixtures/data/minimal_program_signer_append.yaml");

fn write_combine_test_configs(dir: &std::path::Path, cat_asset_id: &str, with_signer: bool) {
    let mut program_text = materialize_minimal_program_text(MinimalProgramParams {
        home_dir: dir,
        ..Default::default()
    });
    if with_signer {
        program_text.push('\n');
        program_text
            .push_str(&MINIMAL_PROGRAM_SIGNER_APPEND.replace("__LAUNCHER_ID__", &"aa".repeat(32)));
    }
    std::fs::write(dir.join("program.yaml"), program_text).expect("write program");
    std::fs::write(
        dir.join("markets.yaml"),
        format!(
            r#"markets:
  - id: dust_m
    enabled: true
    base_asset: "{cat_asset_id}"
    base_symbol: DUST
    quote_asset: xch
    quote_asset_type: unstable
    signer_key_id: key-main-1
    receive_address: {RECEIVE_ADDRESS}
    mode: sell_only
    inventory:
      low_watermark_base_units: 100
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#
        ),
    )
    .expect("write markets");
    std::fs::write(
        dir.join("cats.yaml"),
        format!("cats:\n  - base_symbol: DUST\n    asset_id: \"{cat_asset_id}\"\n"),
    )
    .expect("write cats");
}

fn sample_job(cat_asset_id: &str) -> CatDustJob {
    CatDustJob {
        cat_asset_id: cat_asset_id.to_string(),
        signer_key_id: "key-main-1".to_string(),
        receive_address: RECEIVE_ADDRESS.to_string(),
        market_ids: vec!["dust_m".to_string()],
    }
}

#[tokio::test]
async fn preview_job_report_plans_two_coin_batch_from_simulator_scan() {
    let (scan, _harness) = sim_dust_scan_result(&[400, 300]);
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

    let (dust_count, plan) = plan_dust_for_scan(&scan, 1000, 2);
    assert_eq!(dust_count, 2);
    assert_eq!(plan.combinable_batches.len(), 1);
    assert!(plan.uncombinable.is_empty());

    let report = preview_job_report(&job, &scan, &plan, dust_count, readiness);
    assert_eq!(report.get("status"), Some(&json!("ok")));
    assert_eq!(report.get("combine_batches_planned"), Some(&json!(1)));
    assert_eq!(report.get("uncombinable_dust_count"), Some(&json!(0)));

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
    let (scan, _harness) = sim_dust_scan_result(&[500]);
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
    let (dust_count, plan) = plan_dust_for_scan(&scan, 1000, 2);
    assert_eq!(dust_count, 1);
    assert_eq!(plan.combinable_batches.len(), 0);
    assert_eq!(plan.uncombinable.len(), 1);

    let report = preview_job_report(&job, &scan, &plan, dust_count, readiness);
    let batches = report
        .get("batches")
        .and_then(|value| value.as_array())
        .expect("batches");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].get("status"), Some(&json!("orphan")));
}

#[tokio::test]
async fn preview_job_report_without_signer_backend() {
    let (scan, _harness) = sim_dust_scan_result(&[500]);
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

    let (dust_count, plan) = plan_dust_for_scan(&scan, 1000, 2);
    let report = preview_job_report(&job, &scan, &plan, dust_count, readiness);
    assert_eq!(report.get("signer_config_ok"), Some(&json!(false)));
    assert_eq!(
        report.get("signer_config_note"),
        Some(&json!("signer_not_configured"))
    );
}

#[tokio::test]
async fn dust_batch_coin_ids_resolve_in_simulator() {
    let (scan, harness) = sim_dust_scan_result(&[400, 300]);
    let (_, plan) = plan_dust_for_scan(&scan, 1000, 2);
    for coin in &plan.combinable_batches[0] {
        let coin_id = hex_to_bytes32(&coin.coin_id).expect("coin id");
        let cat = fetch_cat_from_sim_by_id(&harness.chain, coin_id).expect("sim cat");
        assert_eq!(cat.coin.amount, coin.amount);
    }
}

#[tokio::test]
async fn finalize_preview_job_report_end_to_end_from_simulator_scan() {
    let (scan, _harness) = sim_dust_scan_result(&[400, 300]);
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

    let report = finalize_job_report(&job, scan, 1000, 2, JobExecution::Preview, readiness)
        .await
        .expect("preview report");
    assert_eq!(report.get("status"), Some(&json!("ok")));
    assert_eq!(report.get("combine_batches_planned"), Some(&json!(1)));
}

#[tokio::test]
async fn run_combine_emits_json_when_launcher_id_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cat_hex = "f".repeat(64);
    write_combine_test_configs(dir.path(), &cat_hex, false);

    let (output, captured) = ManagerOutput::capturing(true);
    let mgr = ManagerContext::for_test_with_cats(
        dir.path().join("program.yaml"),
        dir.path().join("markets.yaml"),
        dir.path().join("cats.yaml"),
        output,
    );

    let exit = run_combine_market_cat_dust(CombineMarketCatDustRequest {
        mgr: &mgr,
        network: Some("mainnet"),
        coinset_base_url: None,
        launcher_id: None,
        launcher_id_file: None,
        dust_threshold_mojos: 1000,
        max_input_coins: 2,
        max_nonce: 0,
        cat_asset_id: None,
        execution: CombineExecution::from_flags(false, true),
    })
    .await
    .expect("command");

    assert_eq!(exit, 1);
    let payload = captured
        .lock()
        .expect("capture lock")
        .pop()
        .expect("json emitted");
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

    let (output, captured) = ManagerOutput::capturing(true);
    let mgr = ManagerContext::for_test_with_cats(
        dir.path().join("program.yaml"),
        dir.path().join("markets.yaml"),
        dir.path().join("cats.yaml"),
        output,
    );

    let _exit = run_combine_market_cat_dust(CombineMarketCatDustRequest {
        mgr: &mgr,
        network: Some("mainnet"),
        coinset_base_url: None,
        launcher_id: Some(&"aa".repeat(32)),
        launcher_id_file: None,
        dust_threshold_mojos: 1000,
        max_input_coins: 2,
        max_nonce: 0,
        cat_asset_id: None,
        execution: CombineExecution::from_flags(false, true),
    })
    .await
    .expect("command");

    let payload = captured
        .lock()
        .expect("capture lock")
        .pop()
        .expect("json emitted");
    assert_ne!(
        payload.get("reason"),
        Some(&json!("signer_not_configured")),
        "preview must not require signer bundle"
    );
    assert!(payload.get("jobs").is_some());
}

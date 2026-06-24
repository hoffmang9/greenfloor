use super::coinset_context::{load_execution_signer, resolve_combine_coinset_context};
use super::report::{finalize_job_report, preview_job_report, vault_signer_ready, CombineRunMode};
use super::sim_harness::sim_dust_scan_result;
use super::{run_combine_market_cat_dust, CombineExecutionFlags, CombineMarketCatDustRequest};
use crate::coinset::test_support::cat_with_amount;
use crate::coinset::CoinSpentVerifyConfig;
use crate::config::{load_program_config, parse_program_config, read_program_yaml};
use crate::hex::hex_to_bytes32;
use crate::manager_cli::combine_market_cat_dust::jobs::CatDustJob;
use crate::manager_cli::test_support::{
    pop_json, write_combine_test_configs, ManagerContextBuilder,
};
use crate::minimal_program_template::{materialize_minimal_program_text, MinimalProgramParams};
use crate::test_support::simulator::harness::fetch_cat_from_sim_by_id;
use crate::vault_coinset_scan::{dust_coins_from_scan, plan_dust_batches, DustPlan, ScanResult};
use serde_json::json;

const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

fn sample_job(cat_asset_id: &str) -> CatDustJob {
    CatDustJob {
        cat_asset_id: cat_asset_id.to_string(),
        signer_key_id: "key-main-1".to_string(),
        receive_address: RECEIVE_ADDRESS.to_string(),
        market_ids: vec!["dust_m".to_string()],
    }
}

fn test_coinset_context() -> super::coinset_context::CombineCoinsetContext {
    resolve_combine_coinset_context(None, None, "mainnet", "https://api.coinset.org")
}

fn dust_plan_from_scan_without_lineage(
    scan: &ScanResult,
    dust_threshold_mojos: u64,
    max_input_coins: usize,
) -> DustPlan {
    let dust = dust_coins_from_scan(&scan.coins, dust_threshold_mojos);
    let proven: Vec<_> = dust
        .iter()
        .map(|coin| (coin.clone(), cat_with_amount(coin.amount)))
        .collect();
    DustPlan {
        scan_dust_count: dust.len(),
        batches: plan_dust_batches(&proven, max_input_coins),
        lineage_excluded: Vec::new(),
    }
}

fn register_lineage_mocks_for_scan_coins(
    server: &mut mockito::ServerGuard,
    scan: &ScanResult,
    harness: &crate::test_support::simulator::harness::SimulatorVaultHarness,
) {
    use crate::coinset::test_support::{
        coin_record_by_name_request_json, mock_get_coin_record_by_name_body,
        mock_get_puzzle_and_solution_body, mock_unspent_coin_record_by_name_body,
    };
    use crate::test_support::simulator::harness::fetch_cat_from_sim;
    use chia_protocol::CoinSpend;
    use mockito::Matcher;

    let sim = harness.chain.sim.lock().expect("sim lock");
    for row in &scan.coins {
        let coin_id = hex_to_bytes32(&row.coin_id).expect("coin id");
        let coin = sim
            .coin_state(coin_id)
            .map(|state| state.coin)
            .expect("coin state");
        let cat = fetch_cat_from_sim(&sim, coin).expect("sim cat");
        server
            .mock("POST", "/get_coin_record_by_name")
            .match_body(Matcher::PartialJson(coin_record_by_name_request_json(
                cat.coin.coin_id(),
            )))
            .with_status(200)
            .with_body(mock_unspent_coin_record_by_name_body(&cat.coin))
            .create();
        let parent = sim
            .coin_spend(cat.coin.parent_coin_info)
            .expect("parent spend");
        let spent_height = sim
            .coin_state(parent.coin.coin_id())
            .and_then(|state| state.spent_height)
            .unwrap_or(1);
        server
            .mock("POST", "/get_coin_record_by_name")
            .match_body(Matcher::PartialJson(coin_record_by_name_request_json(
                parent.coin.coin_id(),
            )))
            .with_status(200)
            .with_body(mock_get_coin_record_by_name_body(
                &parent.coin,
                spent_height,
            ))
            .create();
        let parent_spend = CoinSpend {
            coin: parent.coin,
            puzzle_reveal: parent.puzzle_reveal.clone(),
            solution: parent.solution.clone(),
        };
        server
            .mock("POST", "/get_puzzle_and_solution")
            .with_status(200)
            .with_body(mock_get_puzzle_and_solution_body(&parent_spend))
            .create();
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

    let plan = dust_plan_from_scan_without_lineage(&scan, 1000, 2);
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
    let plan = dust_plan_from_scan_without_lineage(&scan, 1000, 2);
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

    let plan = dust_plan_from_scan_without_lineage(&scan, 1000, 2);
    let report = preview_job_report(&job, &scan, &test_coinset_context(), &plan, readiness);
    assert_eq!(report.get("signer_config_ok"), Some(&json!(false)));
    assert_eq!(
        report.get("signer_config_note"),
        Some(&json!("signer_not_configured"))
    );
}

#[tokio::test]
async fn dust_batch_coin_ids_resolve_in_simulator() {
    let (scan, harness) = sim_dust_scan_result(&[400, 300]);
    let plan = dust_plan_from_scan_without_lineage(&scan, 1000, 2);
    for coin in &plan.batches.combinable_batches[0].coins {
        let coin_id = hex_to_bytes32(&coin.coin_id).expect("coin id");
        let cat = fetch_cat_from_sim_by_id(&harness.chain, coin_id).expect("sim cat");
        assert_eq!(cat.coin.amount, coin.amount);
    }
}

#[tokio::test]
async fn finalize_preview_job_report_plans_two_coin_batch_with_lineage() {
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

    let mut server = mockito::Server::new_async().await;
    register_lineage_mocks_for_scan_coins(&mut server, &scan, &harness);

    let coinset = resolve_combine_coinset_context(
        None,
        Some(&server.url()),
        "mainnet",
        "https://api.coinset.org",
    );
    let report = finalize_job_report(
        &job,
        scan,
        &coinset,
        1000,
        2,
        &CombineRunMode::Preview,
        readiness,
    )
    .await
    .expect("preview report");
    assert_eq!(report.get("status"), Some(&json!("ok")));
    assert_eq!(report.get("combine_batches_planned"), Some(&json!(1)));
    assert_eq!(report.get("lineage_excluded_dust_count"), Some(&json!(0)));
    assert_eq!(report.get("coinset_base_url"), Some(&json!(server.url())));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn finalize_preview_job_report_marks_unresolvable_coin_lineage_excluded() {
    use super::sim_harness::{coin_row_from_sim_cat, scan_result_from_coin_rows};
    use crate::coinset::test_support::{
        coin_record_by_name_request_json, mock_get_coin_record_by_name_body,
        mock_get_puzzle_and_solution_body, mock_unspent_coin_record_by_name_body,
    };
    use crate::hex::normalize_hex_id;
    use crate::test_support::simulator::harness::SimulatorVaultHarness;
    use crate::vault_coinset_scan::types::CoinKind;
    use chia_protocol::Bytes32;
    use chia_protocol::CoinSpend;
    use mockito::Matcher;

    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let asset_id = normalize_hex_id(&hex::encode(harness.chain.asset_id));
    let launcher_id = normalize_hex_id(&hex::encode(harness.chain.launcher_id));
    let good_cat = harness.fund_vault_cat(400);
    let mut rows = vec![coin_row_from_sim_cat(&good_cat, &asset_id)];
    rows.push(crate::vault_coinset_scan::types::CoinRow {
        coin_id: normalize_hex_id(&hex::encode(Bytes32::new([0xcc; 32]))),
        puzzle_hash: "b".repeat(64),
        parent_coin_info: "c".repeat(64),
        amount: 300,
        confirmed_block_index: 12,
        spent_block_index: 0,
        discovered_nonces: vec![0],
        discovered_by_puzzle_hash: true,
        discovered_by_hint: false,
        kind: CoinKind::Cat,
        cat_asset_id: Some(asset_id.clone()),
        cat_symbols: Vec::new(),
    });
    let scan = scan_result_from_coin_rows(rows, &launcher_id);
    let job = sample_job(&asset_id);
    let dir = tempfile::tempdir().expect("tempdir");
    write_combine_test_configs(dir.path(), &job.cat_asset_id, true);
    let program = load_program_config(&dir.path().join("program.yaml")).expect("program");
    let readiness = vault_signer_ready(&program, &job.signer_key_id);

    let bad_coin_id = hex_to_bytes32(&normalize_hex_id(&hex::encode(Bytes32::new([0xcc; 32]))))
        .expect("bad coin id");
    let (parent_body, puzzle_body, parent_coin_id) = {
        let sim = harness.chain.sim.lock().expect("sim lock");
        let parent = sim
            .coin_spend(good_cat.coin.parent_coin_info)
            .expect("parent spend");
        let spent_height = sim
            .coin_state(parent.coin.coin_id())
            .and_then(|state| state.spent_height)
            .unwrap_or(1);
        let parent_spend = CoinSpend {
            coin: parent.coin,
            puzzle_reveal: parent.puzzle_reveal.clone(),
            solution: parent.solution.clone(),
        };
        (
            mock_get_coin_record_by_name_body(&parent.coin, spent_height),
            mock_get_puzzle_and_solution_body(&parent_spend),
            parent.coin.coin_id(),
        )
    };

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/get_coin_record_by_name")
        .match_body(Matcher::PartialJson(coin_record_by_name_request_json(
            good_cat.coin.coin_id(),
        )))
        .with_status(200)
        .with_body(mock_unspent_coin_record_by_name_body(&good_cat.coin))
        .create();
    server
        .mock("POST", "/get_coin_record_by_name")
        .match_body(Matcher::PartialJson(coin_record_by_name_request_json(
            bad_coin_id,
        )))
        .with_status(200)
        .with_body(json!({"success": true, "coin_record": null}).to_string())
        .create();
    server
        .mock("POST", "/get_coin_record_by_name")
        .match_body(Matcher::PartialJson(coin_record_by_name_request_json(
            parent_coin_id,
        )))
        .with_status(200)
        .with_body(parent_body)
        .create();
    server
        .mock("POST", "/get_puzzle_and_solution")
        .with_status(200)
        .with_body(puzzle_body)
        .create();

    let coinset = resolve_combine_coinset_context(
        None,
        Some(&server.url()),
        "mainnet",
        "https://api.coinset.org",
    );
    let report = finalize_job_report(
        &job,
        scan,
        &coinset,
        1000,
        2,
        &CombineRunMode::Preview,
        readiness,
    )
    .await
    .expect("preview report");
    assert_eq!(report.get("lineage_excluded_dust_count"), Some(&json!(1)));
    assert_eq!(report.get("lineage_proven_dust_count"), Some(&json!(1)));
    assert_eq!(report.get("combine_batches_planned"), Some(&json!(0)));
    let batches = report
        .get("batches")
        .and_then(|value| value.as_array())
        .expect("batches");
    assert_eq!(batches.len(), 2);
    assert!(batches
        .iter()
        .any(|entry| { entry.get("status") == Some(&json!("lineage_excluded")) }));
    assert!(batches
        .iter()
        .any(|entry| { entry.get("status") == Some(&json!("orphan")) }));
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
fn load_execution_signer_applies_coinset_context_overrides() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cat_hex = "f".repeat(64);
    write_combine_test_configs(dir.path(), &cat_hex, true);
    let program_path = dir.path().join("program.yaml");
    let raw = read_program_yaml(&program_path).expect("read program");
    let program = parse_program_config(&raw).expect("parse program");
    let default_coinset =
        super::coinset_context::CombineCoinsetContext::program_default_coinset_base_url(&raw);
    let coinset_ctx = resolve_combine_coinset_context(
        Some("testnet"),
        Some("https://coinset.custom/"),
        &program.network,
        &default_coinset,
    );
    let signer = load_execution_signer(&raw, program, &coinset_ctx).expect("execution signer");
    assert_eq!(signer.network, "testnet11");
    assert_eq!(signer.coinset_msp_base_url, "https://coinset.custom");
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

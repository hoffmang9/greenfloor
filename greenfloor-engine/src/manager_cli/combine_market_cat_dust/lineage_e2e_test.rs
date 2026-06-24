use super::coinset_context::resolve_combine_coinset_context;
use super::combine_test_support::{
    dust_plan_from_scan_without_lineage, register_lineage_mocks_for_scan_coins, sample_job,
};
use super::report::{finalize_job_report, vault_signer_ready, CombineRunMode};
use super::sim_harness::sim_dust_scan_result;
use crate::config::load_program_config;
use crate::hex::{hex_to_bytes32, normalize_hex_id};
use crate::manager_cli::test_support::write_combine_test_configs;
use crate::test_support::simulator::harness::fetch_cat_from_sim_by_id;
use serde_json::json;

#[tokio::test]
async fn dust_batch_coin_ids_resolve_in_simulator() {
    let (scan, harness) = sim_dust_scan_result(&[400, 300]);
    let plan = dust_plan_from_scan_without_lineage(&scan, &harness, 1000, 2);
    for item in &plan.batches.combinable_batches[0].items {
        let dust = item.dust_coin();
        let coin_id = hex_to_bytes32(&dust.coin_id).expect("coin id");
        let cat = fetch_cat_from_sim_by_id(&harness.chain, coin_id).expect("sim cat");
        assert_eq!(cat.coin.amount, dust.amount);
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
    use crate::test_support::simulator::harness::SimulatorVaultHarness;
    use crate::vault_coinset_scan::types::CoinKind;
    use chia_protocol::{Bytes32, CoinSpend};
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
        .any(|entry| entry.get("status") == Some(&json!("lineage_excluded"))));
    assert!(batches
        .iter()
        .any(|entry| entry.get("status") == Some(&json!("orphan"))));
}

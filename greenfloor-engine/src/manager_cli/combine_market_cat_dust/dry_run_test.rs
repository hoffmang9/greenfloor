use super::{run_combine_market_cat_dust, CombineExecution, CombineMarketCatDustRequest};
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::json::ManagerOutput;
use crate::vault_coinset_scan::cat_scan_fixtures::{
    assert_unified_dry_run_batches, build_multi_dust_cat_scan_fixtures, expected_dust_coin_ids,
    mount_cat_scan_mocks, write_combine_dust_test_configs,
};
use serde_json::json;

#[tokio::test]
async fn dry_run_reports_unified_batches_schema_from_mocked_scan() {
    let fixtures = build_multi_dust_cat_scan_fixtures(&[500]);
    let dust_ids = expected_dust_coin_ids(&fixtures);
    let dir = tempfile::tempdir().expect("tempdir");
    write_combine_dust_test_configs(dir.path(), &fixtures);

    let mut server = mockito::Server::new_async().await;
    mount_cat_scan_mocks(&mut server, &fixtures).await;

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
        coinset_base_url: Some(&server.url()),
        launcher_id: Some(&fixtures.launcher_id),
        launcher_id_file: None,
        dust_threshold_mojos: 1000,
        max_input_coins: 2,
        max_nonce: 0,
        cat_asset_id: None,
        execution: CombineExecution::DryRun,
    })
    .await
    .expect("dry run command");

    assert_eq!(exit, 0);
    let payload = captured
        .lock()
        .expect("capture lock")
        .pop()
        .expect("json emitted");
    assert_eq!(payload.get("status"), Some(&json!("ok")));
    assert_eq!(payload.get("dry_run"), Some(&json!(true)));
    assert_unified_dry_run_batches(&payload);

    let orphan_id = payload["jobs"][0]["batches"][0]["coin_ids"][0]
        .as_str()
        .expect("orphan id")
        .to_string();
    assert_eq!(orphan_id, dust_ids[0]);
}

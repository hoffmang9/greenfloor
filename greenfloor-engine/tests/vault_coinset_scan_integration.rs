//! Vault Coinset scan mock Coinset integration tests.

use std::collections::HashSet;

use greenfloor_engine::coinset::{coin_id_from_record, to_coinset_hex};
use greenfloor_engine::hex::normalize_hex_id;
use greenfloor_engine::vault::members::{
    hex_to_bytes32, singleton_member_hash, tree_hash_to_hex, MemberConfig,
};
use greenfloor_engine::vault_coinset_scan::request::ScanRequest;
use greenfloor_engine::vault_coinset_scan::scan::run_vault_coinset_scan;
use greenfloor_engine::vault_coinset_scan::types::{AssetTypeFilter, CoinKind};
use mockito::Matcher;
use serde_json::json;

fn member_p2_hash(launcher_hex: &str, nonce: u32) -> String {
    let launcher = hex_to_bytes32(launcher_hex).expect("launcher bytes");
    let config = MemberConfig::default()
        .with_top_level(true)
        .with_nonce(nonce);
    normalize_hex_id(&tree_hash_to_hex(
        singleton_member_hash(&config, launcher, false).expect("member hash"),
    ))
}

#[tokio::test]
async fn vault_coinset_scan_discovers_xch_coin_via_hint_lookup() {
    let launcher = "11".repeat(32);
    let puzzle_hash = member_p2_hash(&launcher, 0);
    let parent = "22".repeat(32);
    let coin_record = json!({
        "coin": {
            "parent_coin_info": parent,
            "puzzle_hash": puzzle_hash,
            "amount": 1000,
        },
        "confirmed_block_index": 5,
        "spent_block_index": 0,
    });
    let coin_id = coin_id_from_record(&coin_record);
    let coin_name = to_coinset_hex(hex_to_bytes32(&coin_id).expect("coin id bytes").as_ref());

    let parent_name = to_coinset_hex(hex_to_bytes32(&parent).expect("parent bytes").as_ref());
    let coin_records_body =
        serde_json::json!({"success": true, "coin_records": [coin_record.clone()]}).to_string();

    let mut server = mockito::Server::new_async().await;
    let _puzzle_mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hashes")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[]}"#)
        .create_async()
        .await;
    let _hint_mock = server
        .mock("POST", "/get_coin_records_by_hints")
        .with_status(200)
        .with_body(coin_records_body.clone())
        .create_async()
        .await;
    let _parent_mock = server
        .mock("POST", "/get_coin_records_by_names")
        .match_body(Matcher::PartialJson(json!({
            "names": [parent_name.clone()],
            "include_spent_coins": true,
        })))
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[]}"#)
        .create_async()
        .await;
    let _verify_mock = server
        .mock("POST", "/get_coin_records_by_names")
        .match_body(Matcher::PartialJson(json!({
            "names": [coin_name.clone()],
            "include_spent_coins": true,
        })))
        .with_status(200)
        .with_body(coin_records_body)
        .create_async()
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let request = ScanRequest {
        network: "mainnet".to_string(),
        coinset_base_url: Some(server.url()),
        launcher_id: launcher.clone(),
        max_nonce: 0,
        include_spent: false,
        asset_type: AssetTypeFilter::All,
        requested_cat_ids: HashSet::new(),
        requested_cat_tickers: Vec::new(),
        checkpoint_file: None,
        checkpoint_save_interval: 1,
        no_resume_checkpoint: false,
        nonce_batch_size: 32,
        empty_batch_stop_count: 1,
        parent_lookup_batch_size: 64,
        start_height: None,
        end_height: Some(100),
        incremental_from_checkpoint: false,
        auto_increment: false,
        cats_config: dir.path().join("missing-cats.yaml"),
        markets_config: dir.path().join("missing-markets.yaml"),
        testnet_markets_config: None,
        cache_clear: None,
    };

    let result = run_vault_coinset_scan(request)
        .await
        .expect("scan should succeed");
    assert_eq!(result.count, 1);
    assert_eq!(result.launcher_id, launcher);
    assert_eq!(result.coins.len(), 1);
    assert_eq!(result.coins[0].kind, CoinKind::Xch);
    assert_eq!(result.coins[0].coin_id, coin_id);
    let verification = result
        .name_verification
        .expect("verification should run when coins exist");
    assert!(verification.applied);
    assert_eq!(verification.pre_verify_count, 1);
    assert_eq!(verification.verified_count, Some(1));
    assert_eq!(verification.dropped_unverified_count, 0);
}

#[tokio::test]
async fn vault_coinset_scan_drops_unverified_coins() {
    let launcher = "33".repeat(32);
    let puzzle_hash = member_p2_hash(&launcher, 0);
    let parent = "44".repeat(32);
    let coin_record = json!({
        "coin": {
            "parent_coin_info": parent,
            "puzzle_hash": puzzle_hash,
            "amount": 500,
        },
        "confirmed_block_index": 2,
        "spent_block_index": 0,
    });

    let parent = "4".repeat(64);
    let coin_records_body =
        serde_json::json!({"success": true, "coin_records": [coin_record.clone()]}).to_string();
    let parent_name = to_coinset_hex(hex_to_bytes32(&parent).expect("parent bytes").as_ref());

    let mut server = mockito::Server::new_async().await;
    let _puzzle_mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hashes")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[]}"#)
        .create_async()
        .await;
    let _hint_mock = server
        .mock("POST", "/get_coin_records_by_hints")
        .with_status(200)
        .with_body(coin_records_body)
        .create_async()
        .await;
    let _parent_mock = server
        .mock("POST", "/get_coin_records_by_names")
        .match_body(Matcher::PartialJson(json!({
            "names": [parent_name],
            "include_spent_coins": true,
        })))
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[]}"#)
        .create_async()
        .await;
    let _verify_mock = server
        .mock("POST", "/get_coin_records_by_names")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[]}"#)
        .create_async()
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let request = ScanRequest {
        network: "mainnet".to_string(),
        coinset_base_url: Some(server.url()),
        launcher_id: launcher,
        max_nonce: 0,
        include_spent: false,
        asset_type: AssetTypeFilter::All,
        requested_cat_ids: HashSet::new(),
        requested_cat_tickers: Vec::new(),
        checkpoint_file: None,
        checkpoint_save_interval: 1,
        no_resume_checkpoint: false,
        nonce_batch_size: 32,
        empty_batch_stop_count: 1,
        parent_lookup_batch_size: 64,
        start_height: None,
        end_height: Some(50),
        incremental_from_checkpoint: false,
        auto_increment: false,
        cats_config: dir.path().join("missing-cats.yaml"),
        markets_config: dir.path().join("missing-markets.yaml"),
        testnet_markets_config: None,
        cache_clear: None,
    };

    let result = run_vault_coinset_scan(request)
        .await
        .expect("scan should succeed");
    assert_eq!(result.count, 0);
    let verification = result.name_verification.expect("verification applied");
    assert!(verification.applied);
    assert_eq!(verification.pre_verify_count, 1);
    assert_eq!(verification.verified_count, Some(0));
    assert_eq!(verification.dropped_unverified_count, 1);
}

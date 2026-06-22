use std::collections::HashMap;

use super::*;
use crate::vault_coinset_scan::types::CoinKind;

fn sample_row(coin_id: &str) -> crate::vault_coinset_scan::types::CoinRow {
    crate::vault_coinset_scan::types::CoinRow {
        coin_id: coin_id.to_string(),
        puzzle_hash: "b".repeat(64),
        parent_coin_info: "c".repeat(64),
        amount: 1000,
        confirmed_block_index: 10,
        spent_block_index: 0,
        discovered_nonces: vec![1],
        discovered_by_puzzle_hash: true,
        discovered_by_hint: false,
        kind: CoinKind::Xch,
        cat_asset_id: None,
        cat_symbols: vec![],
    }
}

#[test]
fn checkpoint_round_trip_preserves_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("checkpoint.json");
    let launcher = "a".repeat(64);
    let coin_id = "d".repeat(64);
    let mut by_coin_id = HashMap::new();
    by_coin_id.insert(coin_id.clone(), sample_row(&coin_id));
    let mut nonce_to_p2 = HashMap::new();
    nonce_to_p2.insert(1, "b".repeat(64));
    save_scan_checkpoint(&SaveCheckpointParams {
        checkpoint_file: &path,
        network: "mainnet",
        launcher_id: &launcher,
        include_spent: false,
        max_nonce_completed: 1,
        nonce_to_p2: &nonce_to_p2,
        by_coin_id: &by_coin_id,
        cat_asset_cache: &HashMap::new(),
        parent_lineage_cache: &HashMap::new(),
        last_synced_height: Some(100),
        scan_start_height: Some(0),
        scan_end_height: Some(100),
    })
    .expect("save checkpoint");
    let loaded = load_scan_checkpoint(&path, "mainnet", &launcher, false).expect("load");
    assert_eq!(loaded.start_nonce, 2);
    assert_eq!(loaded.last_synced_height, Some(100));
    assert_eq!(loaded.by_coin_id.len(), 1);
    assert!(loaded.by_coin_id.contains_key(&coin_id));
    assert!(!loaded.discarded_mismatch);
}

#[test]
fn checkpoint_mismatch_discards_with_reason_flag() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("checkpoint.json");
    let launcher = "a".repeat(64);
    save_scan_checkpoint(&SaveCheckpointParams {
        checkpoint_file: &path,
        network: "mainnet",
        launcher_id: &launcher,
        include_spent: false,
        max_nonce_completed: 0,
        nonce_to_p2: &HashMap::new(),
        by_coin_id: &HashMap::new(),
        cat_asset_cache: &HashMap::new(),
        parent_lineage_cache: &HashMap::new(),
        last_synced_height: None,
        scan_start_height: None,
        scan_end_height: None,
    })
    .expect("save checkpoint");
    let loaded = load_scan_checkpoint(&path, "testnet11", &launcher, false).expect("load");
    assert_eq!(loaded.start_nonce, 0);
    assert!(loaded.by_coin_id.is_empty());
    assert!(loaded.discarded_mismatch);
}

#[test]
fn checkpoint_invalid_json_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("checkpoint.json");
    std::fs::write(&path, "{not json").expect("write");
    let err =
        load_scan_checkpoint(&path, "mainnet", &"a".repeat(64), false).expect_err("invalid json");
    assert!(err.to_string().contains("parse checkpoint json"));
}

#[test]
fn checkpoint_coin_rows_serialize_type_not_coin_type() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("checkpoint.json");
    let launcher = "a".repeat(64);
    let coin_id = "d".repeat(64);
    let mut by_coin_id = HashMap::new();
    by_coin_id.insert(
        coin_id.clone(),
        crate::vault_coinset_scan::types::CoinRow {
            coin_id: coin_id.clone(),
            puzzle_hash: "b".repeat(64),
            parent_coin_info: "c".repeat(64),
            amount: 1000,
            confirmed_block_index: 10,
            spent_block_index: 0,
            discovered_nonces: vec![1],
            discovered_by_puzzle_hash: true,
            discovered_by_hint: false,
            kind: CoinKind::Cat,
            cat_asset_id: Some("e".repeat(64)),
            cat_symbols: vec![],
        },
    );
    save_scan_checkpoint(&SaveCheckpointParams {
        checkpoint_file: &path,
        network: "mainnet",
        launcher_id: &launcher,
        include_spent: false,
        max_nonce_completed: 0,
        nonce_to_p2: &HashMap::new(),
        by_coin_id: &by_coin_id,
        cat_asset_cache: &HashMap::new(),
        parent_lineage_cache: &HashMap::new(),
        last_synced_height: None,
        scan_start_height: None,
        scan_end_height: None,
    })
    .expect("save checkpoint");
    let raw = std::fs::read_to_string(&path).expect("read checkpoint");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("parse checkpoint");
    let coin_row = &value["coin_rows"][0];
    assert_eq!(coin_row.get("type").and_then(|v| v.as_str()), Some("CAT"));
    assert!(coin_row.get("coin_type").is_none());
}

#[test]
fn checkpoint_loads_legacy_coin_type_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("checkpoint.json");
    let launcher = "a".repeat(64);
    let coin_id = "d".repeat(64);
    std::fs::write(
        &path,
        format!(
            r#"{{
  "version": 1,
  "network": "mainnet",
  "launcher_id": "{launcher}",
  "include_spent": false,
  "max_nonce_completed": 0,
  "scan_window": {{}},
  "nonce_to_p2": {{}},
  "coin_rows": [{{
    "coin_id": "{coin_id}",
    "puzzle_hash": "{}",
    "parent_coin_info": "{}",
    "amount": 1000,
    "confirmed_block_index": 10,
    "spent_block_index": 0,
    "discovered_nonces": [1],
    "discovered_by_puzzle_hash": true,
    "discovered_by_hint": false,
    "coin_type": "XCH",
    "cat_symbols": []
  }}],
  "cat_asset_cache": {{}},
  "parent_lineage_cache": {{}}
}}"#,
            "b".repeat(64),
            "c".repeat(64),
        ),
    )
    .expect("write legacy checkpoint");
    let loaded = load_scan_checkpoint(&path, "mainnet", &launcher, false).expect("load");
    assert_eq!(
        loaded.by_coin_id.get(&coin_id).map(|row| row.kind),
        Some(CoinKind::Xch)
    );
}

#[test]
fn clear_cache_files_reports_missing_and_deleted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let existing = dir.path().join("exists.txt");
    std::fs::write(&existing, "x").expect("write file");
    let missing = dir.path().join("missing.txt");
    let results = clear_cache_files(&[
        existing.display().to_string(),
        missing.display().to_string(),
    ]);
    assert_eq!(
        results.get(&existing.display().to_string()),
        Some(&"deleted".to_string())
    );
    assert_eq!(
        results.get(&missing.display().to_string()),
        Some(&"not_found".to_string())
    );
}

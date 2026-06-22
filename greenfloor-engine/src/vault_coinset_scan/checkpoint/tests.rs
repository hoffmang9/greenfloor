use std::collections::HashMap;
use std::path::Path;

use super::*;
use crate::vault_coinset_scan::types::CoinKind;

fn sample_checkpoint(coin_id: &str) -> LoadedCheckpoint {
    LoadedCheckpoint {
        nonce_to_p2: HashMap::from([(1, "b".repeat(64))]),
        by_coin_id: HashMap::from([(coin_id.to_string(), sample_row(coin_id))]),
        cat_asset_cache: HashMap::new(),
        parent_lineage_cache: HashMap::new(),
        last_synced_height: Some(100),
    }
}

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

fn save_fixture_checkpoint(
    path: &Path,
    launcher: &str,
    network: &str,
    checkpoint: &LoadedCheckpoint,
    max_nonce_completed: u32,
    scan_start_height: Option<u64>,
    scan_end_height: Option<u64>,
) {
    save_scan_checkpoint(
        path,
        &CheckpointWriteMetadata {
            network,
            launcher_id: launcher,
            include_spent: false,
            max_nonce_completed,
            scan_start_height,
            scan_end_height,
        },
        checkpoint,
    )
    .expect("save checkpoint");
}

#[test]
fn checkpoint_round_trip_preserves_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("checkpoint.json");
    let launcher = "a".repeat(64);
    let coin_id = "d".repeat(64);
    let checkpoint = sample_checkpoint(&coin_id);
    save_fixture_checkpoint(
        &path,
        &launcher,
        "mainnet",
        &checkpoint,
        1,
        Some(0),
        Some(100),
    );
    let LoadCheckpointResult::Loaded {
        checkpoint: loaded,
        start_nonce,
    } = load_scan_checkpoint(&path, "mainnet", &launcher, false).expect("load")
    else {
        panic!("expected loaded checkpoint");
    };
    assert_eq!(start_nonce, 2);
    assert_eq!(loaded.last_synced_height, Some(100));
    assert_eq!(loaded.by_coin_id.len(), 1);
    assert!(loaded.by_coin_id.contains_key(&coin_id));
}

#[test]
fn checkpoint_mismatch_discards_with_typed_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("checkpoint.json");
    let launcher = "a".repeat(64);
    save_fixture_checkpoint(
        &path,
        &launcher,
        "mainnet",
        &LoadedCheckpoint::empty(),
        0,
        None,
        None,
    );
    let result = load_scan_checkpoint(&path, "testnet11", &launcher, false).expect("load");
    assert!(matches!(
        result,
        LoadCheckpointResult::Discarded(LoadCheckpointDiscardReason::NetworkMismatch)
    ));
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
    let checkpoint = LoadedCheckpoint {
        nonce_to_p2: HashMap::new(),
        by_coin_id: HashMap::from([(
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
        )]),
        cat_asset_cache: HashMap::new(),
        parent_lineage_cache: HashMap::new(),
        last_synced_height: None,
    };
    save_fixture_checkpoint(&path, &launcher, "mainnet", &checkpoint, 0, None, None);
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
    let LoadCheckpointResult::Loaded {
        checkpoint: loaded, ..
    } = load_scan_checkpoint(&path, "mainnet", &launcher, false).expect("load")
    else {
        panic!("expected loaded checkpoint");
    };
    assert_eq!(
        loaded.by_coin_id.get(&coin_id).map(|row| row.kind),
        Some(CoinKind::Xch)
    );
}

#[test]
fn load_checkpoint_result_empty_is_canonical_default() {
    let LoadCheckpointResult::Loaded {
        checkpoint,
        start_nonce,
    } = LoadCheckpointResult::empty()
    else {
        panic!("expected empty loaded checkpoint");
    };
    assert_eq!(start_nonce, 0);
    assert!(checkpoint.by_coin_id.is_empty());
}

#[test]
fn loaded_checkpoint_max_nonce_scanned() {
    let checkpoint = LoadedCheckpoint {
        nonce_to_p2: HashMap::from([(1, "b".repeat(64)), (5, "c".repeat(64))]),
        ..LoadedCheckpoint::empty()
    };
    assert_eq!(checkpoint.max_nonce_scanned(), 5);
}

#[test]
fn loaded_checkpoint_empty_is_canonical_default() {
    let checkpoint = LoadedCheckpoint::empty();
    assert!(checkpoint.by_coin_id.is_empty());
    assert!(checkpoint.nonce_to_p2.is_empty());
    assert!(checkpoint.cat_asset_cache.is_empty());
    assert!(checkpoint.parent_lineage_cache.is_empty());
    assert!(checkpoint.last_synced_height.is_none());
}

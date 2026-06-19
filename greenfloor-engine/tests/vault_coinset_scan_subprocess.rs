//! Vault Coinset scan integration tests.

#[test]
fn subprocess_vault_coinset_scan_parses_defaults() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args(["vault-coinset-scan", "--help"])
        .output()
        .expect("spawn greenfloor-engine vault-coinset-scan --help");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--launcher-id"));
    assert!(help.contains("--checkpoint-file"));
    assert!(help.contains("--cat-ticker"));
}

#[test]
fn subprocess_vault_coinset_scan_resolve_launcher_id_only_from_arg() {
    let launcher = "ab".repeat(32);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "vault-coinset-scan",
            "--launcher-id",
            &launcher,
            "--resolve-launcher-id-only",
        ])
        .output()
        .expect("spawn vault-coinset-scan resolve launcher from arg");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse resolve launcher json");
    assert_eq!(
        value.get("launcher_id").and_then(|v| v.as_str()),
        Some(launcher.as_str())
    );
    assert_eq!(
        value.get("launcher_id_source").and_then(|v| v.as_str()),
        Some("arg")
    );
}

#[test]
fn subprocess_vault_coinset_scan_resolve_launcher_id_only_requires_config() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "vault-coinset-scan",
            "--program-config",
            "/nonexistent/program.yaml",
            "--resolve-launcher-id-only",
        ])
        .output()
        .expect("spawn vault-coinset-scan resolve launcher");
    assert!(!output.status.success());
}

#[test]
fn vault_coinset_scan_metadata_helpers_in_lib() {
    use greenfloor_engine::vault_coinset_scan::metadata::{
        normalize_label, parse_csv_values, resolve_requested_cat_ids,
    };
    use std::collections::{HashMap, HashSet};

    assert_eq!(normalize_label(" wUSDC.b "), "wusdcb");
    assert_eq!(
        parse_csv_values(&["a,b".to_string()]),
        vec!["a".to_string(), "b".to_string()]
    );
    let mut ticker_map = HashMap::new();
    ticker_map.insert("wusdcb".to_string(), HashSet::from(["aa".repeat(64)]));
    let (resolved, unresolved) =
        resolve_requested_cat_ids(&[], &["wUSDC.b".to_string()], &ticker_map);
    assert!(unresolved.is_empty());
    assert_eq!(resolved.len(), 1);
}

#[test]
fn vault_coinset_scan_checkpoint_round_trip_via_lib() {
    use greenfloor_engine::vault_coinset_scan::checkpoint::{
        load_scan_checkpoint, save_scan_checkpoint, SaveCheckpointParams,
    };
    use greenfloor_engine::vault_coinset_scan::types::{CoinKind, CoinRow};
    use std::collections::HashMap;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("checkpoint.json");
    let launcher = "a".repeat(64);
    let coin_id = "d".repeat(64);
    let mut by_coin_id = HashMap::new();
    by_coin_id.insert(
        coin_id.clone(),
        CoinRow {
            coin_id: coin_id.clone(),
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
        },
    );
    save_scan_checkpoint(&SaveCheckpointParams {
        checkpoint_file: &path,
        network: "mainnet",
        launcher_id: &launcher,
        include_spent: false,
        max_nonce_completed: 1,
        nonce_to_p2: &HashMap::from([(1, "b".repeat(64))]),
        by_coin_id: &by_coin_id,
        cat_asset_cache: &HashMap::new(),
        parent_lineage_cache: &HashMap::new(),
        last_synced_height: Some(100),
        scan_start_height: Some(0),
        scan_end_height: Some(100),
    })
    .expect("save checkpoint");
    let loaded = load_scan_checkpoint(&path, "mainnet", &launcher, false);
    assert_eq!(loaded.start_nonce, 2);
    assert!(loaded.by_coin_id.contains_key(&coin_id));
}

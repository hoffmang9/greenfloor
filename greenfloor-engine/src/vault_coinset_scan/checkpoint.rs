use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::paths::expand_home;
use crate::vault_coinset_scan::types::CoinRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ParentLineageCacheEntry {
    spent_height: u64,
    child_asset_ids: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScanCheckpointFile {
    version: u32,
    network: String,
    launcher_id: String,
    include_spent: bool,
    max_nonce_completed: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_synced_height: Option<u64>,
    scan_window: ScanWindowFields,
    nonce_to_p2: BTreeMap<String, String>,
    coin_rows: Vec<CoinRow>,
    cat_asset_cache: BTreeMap<String, String>,
    parent_lineage_cache: BTreeMap<String, ParentLineageCacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScanWindowFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    start_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_height: Option<u64>,
}

#[derive(Debug)]
pub struct LoadedCheckpoint {
    pub start_nonce: u32,
    pub nonce_to_p2: HashMap<u32, String>,
    pub by_coin_id: HashMap<String, CoinRow>,
    pub cat_asset_cache: HashMap<String, String>,
    pub parent_lineage_cache: HashMap<String, ParentLineageEntry>,
    pub last_synced_height: Option<u64>,
    pub discarded_mismatch: bool,
}

#[derive(Debug, Clone)]
pub struct ParentLineageEntry {
    pub spent_height: u64,
    pub child_asset_ids: HashMap<String, String>,
}

fn empty_checkpoint(discarded_mismatch: bool) -> LoadedCheckpoint {
    LoadedCheckpoint {
        start_nonce: 0,
        nonce_to_p2: HashMap::new(),
        by_coin_id: HashMap::new(),
        cat_asset_cache: HashMap::new(),
        parent_lineage_cache: HashMap::new(),
        last_synced_height: None,
        discarded_mismatch,
    }
}

pub fn clear_cache_files(paths: &[String]) -> BTreeMap<String, String> {
    let mut results = BTreeMap::new();
    for raw_path in paths {
        let clean = raw_path.trim();
        if clean.is_empty() {
            continue;
        }
        let path = expand_home(Path::new(clean));
        let key = path.display().to_string();
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    results.insert(key, "deleted".to_string());
                }
                Err(err) => {
                    results.insert(key, format!("delete_failed:{err}"));
                }
            }
        } else {
            results.insert(key, "not_found".to_string());
        }
    }
    results
}

pub fn load_scan_checkpoint(
    checkpoint_file: &Path,
    network: &str,
    launcher_id: &str,
    include_spent: bool,
) -> SignerResult<LoadedCheckpoint> {
    if !checkpoint_file.exists() {
        return Ok(empty_checkpoint(false));
    }
    let raw = std::fs::read_to_string(checkpoint_file).map_err(|err| {
        SignerError::Other(format!(
            "read checkpoint {}: {err}",
            checkpoint_file.display()
        ))
    })?;
    let parsed: ScanCheckpointFile = serde_json::from_str(&raw).map_err(|err| {
        SignerError::Other(format!(
            "parse checkpoint json {}: {err}",
            checkpoint_file.display()
        ))
    })?;
    if normalize_hex_id(&parsed.launcher_id) != normalize_hex_id(launcher_id) {
        return Ok(empty_checkpoint(true));
    }
    if !parsed.network.trim().eq_ignore_ascii_case(network.trim()) {
        return Ok(empty_checkpoint(true));
    }
    if parsed.include_spent != include_spent {
        return Ok(empty_checkpoint(true));
    }

    let nonce_to_p2 = parsed
        .nonce_to_p2
        .into_iter()
        .filter_map(|(nonce_key, p2_hash)| {
            let nonce = nonce_key.parse::<u32>().ok()?;
            let clean_hash = normalize_hex_id(&p2_hash);
            if clean_hash.is_empty() {
                return None;
            }
            Some((nonce, clean_hash))
        })
        .collect::<HashMap<_, _>>();

    let by_coin_id = parsed
        .coin_rows
        .into_iter()
        .filter_map(|row| {
            let coin_id = normalize_hex_id(&row.coin_id);
            if coin_id.is_empty() {
                return None;
            }
            Some((coin_id, row))
        })
        .collect();

    let cat_asset_cache = parsed
        .cat_asset_cache
        .into_iter()
        .filter_map(|(coin_id_raw, asset_id_raw)| {
            let coin_id = normalize_hex_id(&coin_id_raw);
            if coin_id.is_empty() {
                return None;
            }
            Some((coin_id, normalize_hex_id(&asset_id_raw)))
        })
        .collect();

    let parent_lineage_cache = parsed
        .parent_lineage_cache
        .into_iter()
        .filter_map(|(parent_id_raw, lineage)| {
            let parent_id = normalize_hex_id(&parent_id_raw);
            if parent_id.is_empty() {
                return None;
            }
            let child_asset_ids = lineage
                .child_asset_ids
                .into_iter()
                .filter_map(|(child_id_raw, asset_id_raw)| {
                    let child_id = normalize_hex_id(&child_id_raw);
                    if child_id.is_empty() {
                        return None;
                    }
                    Some((child_id, normalize_hex_id(&asset_id_raw)))
                })
                .collect();
            Some((
                parent_id,
                ParentLineageEntry {
                    spent_height: lineage.spent_height,
                    child_asset_ids,
                },
            ))
        })
        .collect();

    let last_synced_height = parsed.last_synced_height;
    let start_nonce = parsed.max_nonce_completed.saturating_add(1);
    Ok(LoadedCheckpoint {
        start_nonce,
        nonce_to_p2,
        by_coin_id,
        cat_asset_cache,
        parent_lineage_cache,
        last_synced_height,
        discarded_mismatch: false,
    })
}

pub struct SaveCheckpointParams<'a> {
    pub checkpoint_file: &'a Path,
    pub network: &'a str,
    pub launcher_id: &'a str,
    pub include_spent: bool,
    pub max_nonce_completed: u32,
    pub nonce_to_p2: &'a HashMap<u32, String>,
    pub by_coin_id: &'a HashMap<String, CoinRow>,
    pub cat_asset_cache: &'a HashMap<String, String>,
    pub parent_lineage_cache: &'a HashMap<String, ParentLineageEntry>,
    pub last_synced_height: Option<u64>,
    pub scan_start_height: Option<u64>,
    pub scan_end_height: Option<u64>,
}

pub fn save_scan_checkpoint(params: &SaveCheckpointParams<'_>) -> SignerResult<()> {
    if let Some(parent) = params.checkpoint_file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| SignerError::Other(format!("create checkpoint dir: {err}")))?;
    }
    let mut coin_rows: Vec<CoinRow> = params.by_coin_id.values().cloned().collect();
    coin_rows.sort_by(|left, right| left.coin_id.cmp(&right.coin_id));
    for row in &mut coin_rows {
        row.discovered_nonces.sort_unstable();
        row.discovered_nonces.dedup();
        if let Some(asset_id) = row.cat_asset_id.as_ref() {
            row.cat_asset_id = Some(normalize_hex_id(asset_id));
        }
    }
    let payload = ScanCheckpointFile {
        version: 1,
        network: params.network.trim().to_ascii_lowercase(),
        launcher_id: normalize_hex_id(params.launcher_id),
        include_spent: params.include_spent,
        max_nonce_completed: params.max_nonce_completed,
        last_synced_height: params.last_synced_height,
        scan_window: ScanWindowFields {
            start_height: params.scan_start_height,
            end_height: params.scan_end_height,
        },
        nonce_to_p2: params
            .nonce_to_p2
            .iter()
            .map(|(nonce, hash)| (nonce.to_string(), hash.clone()))
            .collect(),
        coin_rows,
        cat_asset_cache: params
            .cat_asset_cache
            .iter()
            .map(|(coin_id, asset_id)| (coin_id.clone(), asset_id.clone()))
            .collect(),
        parent_lineage_cache: params
            .parent_lineage_cache
            .iter()
            .map(|(parent_id, lineage)| {
                (
                    parent_id.clone(),
                    ParentLineageCacheEntry {
                        spent_height: lineage.spent_height,
                        child_asset_ids: lineage
                            .child_asset_ids
                            .iter()
                            .map(|(child_id, asset_id)| (child_id.clone(), asset_id.clone()))
                            .collect(),
                    },
                )
            })
            .collect(),
    };
    std::fs::write(
        params.checkpoint_file,
        serde_json::to_string_pretty(&payload)
            .map_err(|err| SignerError::Other(format!("encode checkpoint json: {err}")))?,
    )
    .map_err(|err| SignerError::Other(format!("write checkpoint: {err}")))?;
    Ok(())
}

pub fn write_launcher_id_file(path: &Path, launcher_id: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{launcher_id}\n"))
}

pub fn read_launcher_id_file(path: &Path) -> SignerResult<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!("read launcher id file {}: {err}", path.display()))
    })?;
    Ok(normalize_hex_id(raw.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_coinset_scan::types::CoinKind;

    fn sample_row(coin_id: &str) -> CoinRow {
        CoinRow {
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
        let err = load_scan_checkpoint(&path, "mainnet", &"a".repeat(64), false)
            .expect_err("invalid json");
        assert!(err.to_string().contains("parse checkpoint json"));
    }

    #[test]
    fn read_launcher_id_file_errors_when_unreadable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("launcher.txt");
        std::fs::write(&path, "ab".repeat(32)).expect("write launcher id");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
            perms.set_mode(0o000);
            std::fs::set_permissions(&path, perms).expect("chmod");
            let err = read_launcher_id_file(&path).expect_err("unreadable launcher id file");
            assert!(err.to_string().contains("read launcher id file"));
        }
    }

    #[test]
    fn read_launcher_id_file_returns_empty_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("missing.txt");
        assert!(read_launcher_id_file(&path)
            .expect("missing file")
            .is_empty());
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
}

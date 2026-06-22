use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::vault_coinset_scan::types::CoinRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentLineageEntry {
    pub spent_height: u64,
    pub child_asset_ids: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ScanCheckpointFile {
    pub(crate) version: u32,
    pub(crate) network: String,
    pub(crate) launcher_id: String,
    pub(crate) include_spent: bool,
    pub(crate) max_nonce_completed: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_synced_height: Option<u64>,
    pub(crate) scan_window: ScanWindowFields,
    pub(crate) nonce_to_p2: BTreeMap<String, String>,
    pub(crate) coin_rows: Vec<CoinRow>,
    pub(crate) cat_asset_cache: BTreeMap<String, String>,
    pub(crate) parent_lineage_cache: BTreeMap<String, ParentLineageEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ScanWindowFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) end_height: Option<u64>,
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

pub struct SaveCheckpointParams<'a> {
    pub checkpoint_file: &'a std::path::Path,
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

pub(crate) fn normalize_lineage_entry(mut entry: ParentLineageEntry) -> ParentLineageEntry {
    use crate::hex::normalize_hex_id;

    entry.child_asset_ids = entry
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
    entry
}

pub(crate) fn empty_checkpoint(discarded_mismatch: bool) -> LoadedCheckpoint {
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

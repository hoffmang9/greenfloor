use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::hex::normalize_hex_id;
use crate::vault_coinset_scan::types::CoinRow;

use super::runtime::{LoadCheckpointDiscardReason, LoadedCheckpoint, ParentLineageEntry};

#[derive(Debug, Clone)]
pub struct CheckpointWriteMetadata<'a> {
    pub network: &'a str,
    pub launcher_id: &'a str,
    pub include_spent: bool,
    pub max_nonce_completed: u32,
    pub scan_start_height: Option<u64>,
    pub scan_end_height: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ScanCheckpointFile {
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
    parent_lineage_cache: BTreeMap<String, ParentLineageEntryOnDisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScanWindowFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    start_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_height: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ParentLineageEntryOnDisk {
    spent_height: u64,
    child_asset_ids: BTreeMap<String, String>,
}

impl ScanCheckpointFile {
    pub(crate) fn validate_params(
        &self,
        network: &str,
        launcher_id: &str,
        include_spent: bool,
    ) -> Result<(), LoadCheckpointDiscardReason> {
        if normalize_hex_id(&self.launcher_id) != normalize_hex_id(launcher_id) {
            return Err(LoadCheckpointDiscardReason::LauncherIdMismatch);
        }
        if !self.network.trim().eq_ignore_ascii_case(network.trim()) {
            return Err(LoadCheckpointDiscardReason::NetworkMismatch);
        }
        if self.include_spent != include_spent {
            return Err(LoadCheckpointDiscardReason::IncludeSpentMismatch);
        }
        Ok(())
    }

    pub(crate) fn into_loaded(self) -> (LoadedCheckpoint, u32) {
        (
            LoadedCheckpoint {
                nonce_to_p2: normalize_nonce_map(self.nonce_to_p2),
                by_coin_id: normalize_coin_rows(self.coin_rows),
                cat_asset_cache: normalize_hex_keyed_map(self.cat_asset_cache),
                parent_lineage_cache: normalize_lineage_cache(self.parent_lineage_cache),
                last_synced_height: self.last_synced_height,
            },
            self.max_nonce_completed.saturating_add(1),
        )
    }

    pub(crate) fn from_loaded(
        checkpoint: &LoadedCheckpoint,
        metadata: &CheckpointWriteMetadata<'_>,
    ) -> Self {
        let mut coin_rows: Vec<CoinRow> = checkpoint.by_coin_id.values().cloned().collect();
        coin_rows.sort_by(|left, right| left.coin_id.cmp(&right.coin_id));
        for row in &mut coin_rows {
            row.discovered_nonces.sort_unstable();
            row.discovered_nonces.dedup();
            if let Some(asset_id) = row.cat_asset_id.as_ref() {
                row.cat_asset_id = Some(normalize_hex_id(asset_id));
            }
        }

        Self {
            version: 1,
            network: metadata.network.trim().to_ascii_lowercase(),
            launcher_id: normalize_hex_id(metadata.launcher_id),
            include_spent: metadata.include_spent,
            max_nonce_completed: metadata.max_nonce_completed,
            last_synced_height: checkpoint.last_synced_height,
            scan_window: ScanWindowFields {
                start_height: metadata.scan_start_height,
                end_height: metadata.scan_end_height,
            },
            nonce_to_p2: checkpoint
                .nonce_to_p2
                .iter()
                .map(|(nonce, hash)| (nonce.to_string(), hash.clone()))
                .collect(),
            coin_rows,
            cat_asset_cache: string_map_to_btree(&checkpoint.cat_asset_cache),
            parent_lineage_cache: checkpoint
                .parent_lineage_cache
                .iter()
                .map(|(parent_id, lineage)| {
                    (
                        parent_id.clone(),
                        ParentLineageEntryOnDisk {
                            spent_height: lineage.spent_height,
                            child_asset_ids: string_map_to_btree(&lineage.child_asset_ids),
                        },
                    )
                })
                .collect(),
        }
    }
}

fn string_map_to_btree(map: &HashMap<String, String>) -> BTreeMap<String, String> {
    map.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn normalize_hex_keyed_map(
    entries: impl IntoIterator<Item = (String, String)>,
) -> HashMap<String, String> {
    entries
        .into_iter()
        .filter_map(|(key_raw, value_raw)| {
            let key = normalize_hex_id(&key_raw);
            if key.is_empty() {
                return None;
            }
            Some((key, normalize_hex_id(&value_raw)))
        })
        .collect()
}

fn normalize_nonce_map(entries: BTreeMap<String, String>) -> HashMap<u32, String> {
    entries
        .into_iter()
        .filter_map(|(nonce_key, p2_hash)| {
            let nonce = nonce_key.parse::<u32>().ok()?;
            let clean_hash = normalize_hex_id(&p2_hash);
            if clean_hash.is_empty() {
                return None;
            }
            Some((nonce, clean_hash))
        })
        .collect()
}

fn normalize_coin_rows(rows: Vec<CoinRow>) -> HashMap<String, CoinRow> {
    rows.into_iter()
        .filter_map(|row| {
            let coin_id = normalize_hex_id(&row.coin_id);
            if coin_id.is_empty() {
                return None;
            }
            Some((coin_id, row))
        })
        .collect()
}

fn normalize_lineage_entry(entry: ParentLineageEntryOnDisk) -> ParentLineageEntry {
    ParentLineageEntry {
        spent_height: entry.spent_height,
        child_asset_ids: normalize_hex_keyed_map(entry.child_asset_ids),
    }
}

fn normalize_lineage_cache(
    entries: BTreeMap<String, ParentLineageEntryOnDisk>,
) -> HashMap<String, ParentLineageEntry> {
    entries
        .into_iter()
        .filter_map(|(parent_id_raw, lineage)| {
            let parent_id = normalize_hex_id(&parent_id_raw);
            if parent_id.is_empty() {
                return None;
            }
            Some((parent_id, normalize_lineage_entry(lineage)))
        })
        .collect()
}

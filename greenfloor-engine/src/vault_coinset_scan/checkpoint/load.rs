use std::collections::HashMap;
use std::path::Path;

use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;

use super::types::{
    empty_checkpoint, normalize_lineage_entry, LoadedCheckpoint, ScanCheckpointFile,
};

/// Load scan checkpoint.
///
/// # Errors
///
/// Returns an error if the operation fails.
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
            Some((parent_id, normalize_lineage_entry(lineage)))
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

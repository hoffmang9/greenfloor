use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::vault_coinset_scan::types::CoinRow;

use super::types::{SaveCheckpointParams, ScanCheckpointFile, ScanWindowFields};

/// Save scan checkpoint.
///
/// # Errors
///
/// Returns an error if the operation fails.
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
            .map(|(parent_id, lineage)| (parent_id.clone(), lineage.clone()))
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

//! CLI: discover (and optionally reclaim) orphaned vault-hinted presplit makers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::coinset::client_for_signer_on_network;
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, normalize_hex_id};
use crate::offer::lifecycle::reclaim_cli::{
    confirm_wait_for_run, offers_reclaim_presplit_pairs, OffersReclaimPresplitCliResult,
    PresplitReclaimPair,
};
use crate::offer::presplit::{
    discover_orphan_presplit_candidates, OrphanPresplitCandidate, OrphanScanRow,
};
use crate::storage::{resolve_state_db_path, SqliteStore};

/// Request for orphaned-presplit discovery / reclaim.
#[derive(Debug, Clone)]
pub struct OffersOrphanPresplitCliRequest {
    pub asset_id: String,
    pub market_id: Option<String>,
    pub start_height: Option<u64>,
    pub reclaim: bool,
    pub dry_run: bool,
    pub wait: bool,
    pub home_dir: PathBuf,
    pub state_db_override: Option<String>,
    pub launcher_id: String,
    pub scan_rows: Vec<OrphanScanRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OffersOrphanPresplitCliItem {
    pub coin_id: String,
    pub amount: u64,
    /// CAT display units (1000 mojos = 1 unit). Integer units only for CLI summary.
    pub units: u64,
    pub confirmed_block_index: u64,
    pub fixed_delegated_puzzle_hash: Option<String>,
    pub recovery: &'static str,
    pub recovery_detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OffersOrphanPresplitCliResult {
    pub op: &'static str,
    pub asset: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_id: Option<String>,
    pub launcher_id: String,
    pub start_height: Option<u64>,
    pub reclaim: bool,
    pub dry_run: bool,
    pub wait: bool,
    pub discovered_count: u64,
    pub reclaimable_count: u64,
    pub unreclaimable_count: u64,
    pub orphans: Vec<OffersOrphanPresplitCliItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reclaim_result: Option<OffersReclaimPresplitCliResult>,
}

fn tracked_cancel_input_coin_ids(
    home_dir: &Path,
    state_db_override: Option<&str>,
) -> SignerResult<HashSet<String>> {
    // Always exclude every tracked maker. `--market-id` scopes asset resolution / reporting
    // only — narrowing the exclusion set could reclaim another market's open maker.
    let db_path = resolve_state_db_path(home_dir, state_db_override);
    let store = SqliteStore::open(&db_path)?;
    let rows = store.list_unreturned_presplit_makers(None)?;
    Ok(rows
        .into_iter()
        .map(|row| normalize_hex_id(&row.cancel_input_coin_id))
        .filter(|id| id.len() == 64)
        .collect())
}

fn item_from_candidate(candidate: &OrphanPresplitCandidate) -> OffersOrphanPresplitCliItem {
    OffersOrphanPresplitCliItem {
        coin_id: candidate.coin_id.clone(),
        amount: candidate.amount,
        units: candidate.amount / 1000,
        confirmed_block_index: candidate.confirmed_block_index,
        fixed_delegated_puzzle_hash: candidate.fixed_delegated_puzzle_hash.clone(),
        recovery: candidate.recovery_label(),
        recovery_detail: candidate.recovery_detail.clone(),
    }
}

fn reclaimable_pairs(candidates: &[OrphanPresplitCandidate]) -> Vec<PresplitReclaimPair> {
    candidates
        .iter()
        .filter_map(|candidate| {
            candidate
                .fixed_delegated_puzzle_hash
                .as_ref()
                .map(|fixed| PresplitReclaimPair {
                    coin_id: candidate.coin_id.clone(),
                    fixed_delegated_puzzle_hash: fixed.clone(),
                })
        })
        .collect()
}

/// Discover orphaned vault-hinted presplit makers; optionally reclaim recoverable ones.
///
/// # Errors
///
/// Returns an error when `SQLite`, Coinset transport, or reclaim orchestration fails.
pub async fn offers_orphan_presplit_cli(
    signer: SignerConfig,
    network: &str,
    request: OffersOrphanPresplitCliRequest,
) -> SignerResult<OffersOrphanPresplitCliResult> {
    let asset = normalize_hex_id(&request.asset_id);
    if asset.len() != 64 {
        return Err(SignerError::Other(
            "--asset must resolve to a 64-char CAT asset id".to_string(),
        ));
    }
    let launcher = normalize_hex_id(&request.launcher_id);
    if launcher.len() != 64 {
        return Err(SignerError::Other(
            "launcher id must be 64 hex characters".to_string(),
        ));
    }
    let launcher_id = hex_to_bytes32(&launcher)?;
    let tracked =
        tracked_cancel_input_coin_ids(&request.home_dir, request.state_db_override.as_deref())?;
    let client = client_for_signer_on_network(&signer, network)?;
    let candidates = discover_orphan_presplit_candidates(
        &client,
        launcher_id,
        &request.scan_rows,
        &asset,
        &tracked,
        request.start_height,
    )
    .await?;

    let orphans: Vec<OffersOrphanPresplitCliItem> =
        candidates.iter().map(item_from_candidate).collect();
    let pairs = reclaimable_pairs(&candidates);
    let unreclaimable_count =
        u64::try_from(candidates.len().saturating_sub(pairs.len())).unwrap_or(u64::MAX);

    let reclaim_result = if request.reclaim {
        Some(
            offers_reclaim_presplit_pairs(
                signer,
                network,
                &pairs,
                request.dry_run,
                confirm_wait_for_run(request.dry_run, request.wait),
            )
            .await?,
        )
    } else {
        None
    };

    Ok(OffersOrphanPresplitCliResult {
        op: "offers-orphan-presplit",
        asset,
        market_id: request.market_id,
        launcher_id: launcher,
        start_height: request.start_height,
        reclaim: request.reclaim,
        dry_run: request.dry_run,
        wait: request.wait,
        discovered_count: u64::try_from(candidates.len()).unwrap_or(u64::MAX),
        reclaimable_count: u64::try_from(pairs.len()).unwrap_or(u64::MAX),
        unreclaimable_count,
        orphans,
        reclaim_result,
    })
}

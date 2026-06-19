use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use crate::coinset::DirectCoinsetScanClient;
use crate::vault_coinset_scan::checkpoint::ParentLineageEntry;
use crate::vault_coinset_scan::request::ScanRequest;
use crate::vault_coinset_scan::types::{AssetTypeFilter, CoinKind, CoinRow, ScanStopReason};
use crate::vault_coinset_scan::window::ScanWindowPlan;

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub network: String,
    pub coinset_base_url: Option<String>,
    pub launcher_id: String,
    pub asset_type: AssetTypeFilter,
    pub requested_cat_ids: Vec<String>,
    pub requested_cat_tickers: Vec<String>,
    pub max_nonce_scanned: u32,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_verification: Option<NameVerification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_clear: Option<BTreeMap<String, String>>,
    pub checkpoint: CheckpointSummary,
    pub scan_batches: ScanBatchConfig,
    pub scan_window: ScanWindowSummary,
    pub scan_stop_reason: ScanStopReason,
    pub coins: Vec<OutputCoinRow>,
}

#[derive(Debug, Serialize)]
pub struct NameVerification {
    pub applied: bool,
    pub pre_verify_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_count: Option<usize>,
    pub dropped_unverified_count: usize,
}

#[derive(Debug, Serialize)]
pub struct CheckpointSummary {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub resumed: bool,
    pub start_nonce: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_interval: Option<u32>,
    pub cat_asset_cache_entries: usize,
    pub parent_lineage_cache_entries: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_synced_height: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ScanBatchConfig {
    pub nonce_batch_size: u32,
    pub empty_batch_stop_count: u32,
    pub parent_lookup_batch_size: u32,
}

#[derive(Debug, Serialize)]
pub struct ScanWindowSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_peak_height: Option<u64>,
    pub incremental_from_checkpoint: bool,
    pub auto_increment: bool,
}

#[derive(Debug, Serialize)]
pub struct OutputCoinRow {
    pub coin_id: String,
    #[serde(rename = "type")]
    pub kind: CoinKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cat_asset_id: Option<String>,
    pub cat_symbols: Vec<String>,
    pub amount: u64,
    pub confirmed_block_index: u64,
    pub spent_block_index: u64,
    pub discovered_nonces: Vec<u32>,
    pub discovered_by_puzzle_hash: bool,
    pub discovered_by_hint: bool,
    pub puzzle_hash: String,
    pub parent_coin_info: String,
}

impl From<CoinRow> for OutputCoinRow {
    fn from(row: CoinRow) -> Self {
        Self {
            coin_id: row.coin_id,
            kind: row.kind,
            cat_asset_id: row.cat_asset_id,
            cat_symbols: row.cat_symbols,
            amount: row.amount,
            confirmed_block_index: row.confirmed_block_index,
            spent_block_index: row.spent_block_index,
            discovered_nonces: row.discovered_nonces,
            discovered_by_puzzle_hash: row.discovered_by_puzzle_hash,
            discovered_by_hint: row.discovered_by_hint,
            puzzle_hash: row.puzzle_hash,
            parent_coin_info: row.parent_coin_info,
        }
    }
}

pub struct ScanResultParams<'a> {
    pub scanner: &'a DirectCoinsetScanClient,
    pub request: &'a ScanRequest,
    pub launcher_id: &'a str,
    pub effective_asset_type: AssetTypeFilter,
    pub requested_cat_ids: &'a HashSet<String>,
    pub nonce_to_p2: &'a HashMap<u32, String>,
    pub cat_asset_cache: &'a HashMap<String, String>,
    pub parent_lineage_cache: &'a HashMap<String, ParentLineageEntry>,
    pub checkpoint_enabled: bool,
    pub checkpoint_resumed: bool,
    pub checkpoint_start_nonce: u32,
    pub window: &'a ScanWindowPlan,
    pub stop_reason: ScanStopReason,
    pub filtered: Vec<CoinRow>,
    pub name_verification: Option<NameVerification>,
}

impl ScanResult {
    pub fn build(params: ScanResultParams<'_>) -> Self {
        let max_nonce_scanned = params.nonce_to_p2.keys().copied().max().unwrap_or(0);
        let mut requested_cat_ids: Vec<String> = params.requested_cat_ids.iter().cloned().collect();
        requested_cat_ids.sort();
        let mut requested_cat_tickers = params.request.requested_cat_tickers.clone();
        requested_cat_tickers.sort();
        requested_cat_tickers.dedup();

        Self {
            network: params.scanner.network.clone(),
            coinset_base_url: params.scanner.base_url.clone(),
            launcher_id: params.launcher_id.to_string(),
            asset_type: params.effective_asset_type,
            requested_cat_ids,
            requested_cat_tickers,
            max_nonce_scanned,
            count: params.filtered.len(),
            name_verification: params.name_verification,
            cache_clear: params.request.cache_clear.clone(),
            checkpoint: CheckpointSummary {
                enabled: params.checkpoint_enabled,
                file: params
                    .request
                    .checkpoint_file
                    .as_ref()
                    .map(|path| path.display().to_string()),
                resumed: params.checkpoint_resumed,
                start_nonce: params.checkpoint_start_nonce,
                save_interval: if params.checkpoint_enabled {
                    Some(params.request.checkpoint_save_interval)
                } else {
                    None
                },
                cat_asset_cache_entries: params.cat_asset_cache.len(),
                parent_lineage_cache_entries: params.parent_lineage_cache.len(),
                last_synced_height: params.window.checkpoint_synced_height,
            },
            scan_batches: ScanBatchConfig {
                nonce_batch_size: params.request.nonce_batch_size,
                empty_batch_stop_count: params.request.empty_batch_stop_count,
                parent_lookup_batch_size: params.request.parent_lookup_batch_size,
            },
            scan_window: ScanWindowSummary {
                start_height: params.window.effective_start_height,
                end_height: params.window.effective_end_height,
                chain_peak_height: params.window.chain_peak_height,
                incremental_from_checkpoint: params.request.incremental_from_checkpoint,
                auto_increment: params.request.auto_increment,
            },
            scan_stop_reason: params.stop_reason,
            coins: params
                .filtered
                .into_iter()
                .map(OutputCoinRow::from)
                .collect(),
        }
    }
}

pub fn filter_rows(
    by_coin_id: &HashMap<String, CoinRow>,
    asset_type: AssetTypeFilter,
    requested_cat_ids: &HashSet<String>,
) -> Vec<CoinRow> {
    let mut filtered: Vec<CoinRow> = by_coin_id.values().cloned().collect();
    filtered.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.amount.cmp(&right.amount))
            .then_with(|| left.coin_id.cmp(&right.coin_id))
    });
    filtered
        .into_iter()
        .filter(|row| match asset_type {
            AssetTypeFilter::Xch => row.kind.is_xch(),
            AssetTypeFilter::Cat => row.kind.is_cat(),
            AssetTypeFilter::All => true,
        })
        .filter(|row| {
            requested_cat_ids.is_empty()
                || requested_cat_ids.contains(row.cat_asset_id.as_deref().unwrap_or(""))
        })
        .collect()
}

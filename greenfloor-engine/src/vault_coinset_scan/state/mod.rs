use std::collections::{HashMap, HashSet};

use crate::error::SignerResult;
use crate::vault_coinset_scan::cat_detect::{classify_coin_rows, CatDetectCaches};
use crate::vault_coinset_scan::checkpoint::save_scan_checkpoint;
use crate::vault_coinset_scan::checkpoint::{ParentLineageEntry, SaveCheckpointParams};
use crate::vault_coinset_scan::request::ScanRequest;
use crate::vault_coinset_scan::result::{
    apply_name_verification, filter_rows, CheckpointSummary, NameVerification, ScanBatchConfig,
    ScanResult, ScanWindowSummary,
};
use crate::vault_coinset_scan::types::{CoinRow, ScanStopReason};

mod nonce_scan;
mod prepare;

#[cfg(test)]
mod cat_scan_test;

use prepare::{
    load_checkpoint_or_default, resolve_effective_window, resolve_scan_client,
    resolve_scan_metadata,
};

pub struct ScanState {
    request: ScanRequest,
    scanner: crate::coinset::DirectCoinsetScanClient,
    launcher_id: String,
    launcher_bytes: chia_protocol::Bytes32,
    effective_asset_type: crate::vault_coinset_scan::types::AssetTypeFilter,
    requested_cat_ids: HashSet<String>,
    asset_id_to_symbols: std::collections::BTreeMap<String, Vec<String>>,
    checkpoint_enabled: bool,
    checkpoint_resumed: bool,
    checkpoint_discarded_mismatch: bool,
    checkpoint_start_nonce: u32,
    nonce_to_p2: HashMap<u32, String>,
    by_coin_id: HashMap<String, CoinRow>,
    cat_asset_cache: HashMap<String, String>,
    parent_lineage_cache: HashMap<String, ParentLineageEntry>,
    window: crate::vault_coinset_scan::window::ScanWindowPlan,
    stop_reason: ScanStopReason,
}

impl ScanState {
    pub async fn run(request: ScanRequest) -> SignerResult<ScanResult> {
        let mut state = Self::prepare(request).await?;
        if state.window.exhausted {
            state.stop_reason = ScanStopReason::ScanWindowExhausted;
            return Ok(state.finish(Vec::new(), None));
        }
        state.scan_nonces().await?;
        state.classify_rows().await?;
        let name_verification = state.verify_and_filter_names().await?;
        state.save_final_checkpoint()?;
        let filtered = filter_rows(
            &state.by_coin_id,
            state.effective_asset_type,
            &state.requested_cat_ids,
        );
        Ok(state.finish(filtered, name_verification))
    }

    async fn prepare(request: ScanRequest) -> SignerResult<Self> {
        let ScanMetadata {
            effective_asset_type,
            requested_cat_ids,
            asset_id_to_symbols,
        } = resolve_scan_metadata(&request)?;
        let ResolvedScanClient {
            scanner,
            launcher_id,
            launcher_bytes,
        } = resolve_scan_client(&request)?;

        let checkpoint_enabled = request.checkpoint_file.is_some();
        let loaded = load_checkpoint_or_default(&request, &scanner.network, &launcher_id)?;
        let checkpoint_discarded_mismatch = loaded.discarded_mismatch;
        let checkpoint_resumed = !loaded.discarded_mismatch
            && (loaded.start_nonce > 0
                || !loaded.by_coin_id.is_empty()
                || !loaded.cat_asset_cache.is_empty()
                || !loaded.parent_lineage_cache.is_empty());
        let crate::vault_coinset_scan::checkpoint::LoadedCheckpoint {
            start_nonce: checkpoint_start_nonce,
            nonce_to_p2,
            by_coin_id,
            cat_asset_cache,
            parent_lineage_cache,
            last_synced_height: checkpoint_last_synced_height,
            ..
        } = loaded;

        let window = resolve_effective_window(
            &scanner,
            &request,
            checkpoint_enabled,
            checkpoint_last_synced_height,
        )
        .await?;

        Ok(Self {
            request,
            scanner,
            launcher_id,
            launcher_bytes,
            effective_asset_type,
            requested_cat_ids,
            asset_id_to_symbols,
            checkpoint_enabled,
            checkpoint_resumed,
            checkpoint_discarded_mismatch,
            checkpoint_start_nonce,
            nonce_to_p2,
            by_coin_id,
            cat_asset_cache,
            parent_lineage_cache,
            window,
            stop_reason: ScanStopReason::MaxNonceReached,
        })
    }

    async fn classify_rows(&mut self) -> SignerResult<()> {
        let mut detect_caches = CatDetectCaches::new(
            std::mem::take(&mut self.cat_asset_cache),
            std::mem::take(&mut self.parent_lineage_cache),
        );

        classify_coin_rows(
            &self.scanner,
            &mut self.by_coin_id,
            &self.nonce_to_p2,
            &self.asset_id_to_symbols,
            self.request.parent_lookup_batch_size,
            &mut detect_caches,
        )
        .await?;

        self.cat_asset_cache = detect_caches.cat_asset_cache;
        self.parent_lineage_cache = detect_caches.parent_lineage_cache;
        Ok(())
    }

    async fn verify_and_filter_names(&mut self) -> SignerResult<Option<NameVerification>> {
        if self.by_coin_id.is_empty() {
            return Ok(None);
        }
        let verified_coin_ids = self
            .scanner
            .existing_coin_names(&self.by_coin_id.keys().cloned().collect::<Vec<_>>())
            .await?;
        Ok(apply_name_verification(
            &mut self.by_coin_id,
            &verified_coin_ids,
        ))
    }

    fn save_final_checkpoint(&self) -> SignerResult<()> {
        if !self.checkpoint_enabled {
            return Ok(());
        }
        let max_nonce_scanned = self.nonce_to_p2.keys().copied().max().unwrap_or(0);
        self.write_checkpoint(max_nonce_scanned)
    }

    pub(super) fn write_checkpoint(&self, max_nonce_completed: u32) -> SignerResult<()> {
        save_scan_checkpoint(&SaveCheckpointParams {
            checkpoint_file: self
                .request
                .checkpoint_file
                .as_ref()
                .expect("checkpoint file"),
            network: &self.scanner.network,
            launcher_id: &self.launcher_id,
            include_spent: self.request.include_spent,
            max_nonce_completed,
            nonce_to_p2: &self.nonce_to_p2,
            by_coin_id: &self.by_coin_id,
            cat_asset_cache: &self.cat_asset_cache,
            parent_lineage_cache: &self.parent_lineage_cache,
            last_synced_height: self.window.checkpoint_synced_height,
            scan_start_height: self.window.effective_start_height,
            scan_end_height: self.window.effective_end_height,
        })
    }

    fn finish(
        self,
        filtered: Vec<CoinRow>,
        name_verification: Option<NameVerification>,
    ) -> ScanResult {
        let max_nonce_scanned = self.nonce_to_p2.keys().copied().max().unwrap_or(0);
        let mut requested_cat_ids: Vec<String> = self.requested_cat_ids.into_iter().collect();
        requested_cat_ids.sort();
        let mut requested_cat_tickers = self.request.requested_cat_tickers;
        requested_cat_tickers.sort();
        requested_cat_tickers.dedup();

        ScanResult {
            network: self.scanner.network,
            coinset_base_url: self.scanner.base_url,
            launcher_id: self.launcher_id,
            asset_type: self.effective_asset_type,
            requested_cat_ids,
            requested_cat_tickers,
            max_nonce_scanned,
            count: filtered.len(),
            name_verification,
            cache_clear: self.request.cache_clear,
            checkpoint: CheckpointSummary {
                enabled: self.checkpoint_enabled,
                file: self
                    .request
                    .checkpoint_file
                    .map(|path| path.display().to_string()),
                resumed: self.checkpoint_resumed,
                start_nonce: self.checkpoint_start_nonce,
                save_interval: if self.checkpoint_enabled {
                    Some(self.request.checkpoint_save_interval)
                } else {
                    None
                },
                cat_asset_cache_entries: self.cat_asset_cache.len(),
                parent_lineage_cache_entries: self.parent_lineage_cache.len(),
                last_synced_height: self.window.checkpoint_synced_height,
                discard_reason: if self.checkpoint_discarded_mismatch {
                    Some("checkpoint_params_mismatch".to_string())
                } else {
                    None
                },
            },
            scan_batches: ScanBatchConfig {
                nonce_batch_size: self.request.nonce_batch_size,
                empty_batch_stop_count: self.request.empty_batch_stop_count,
                parent_lookup_batch_size: self.request.parent_lookup_batch_size,
            },
            scan_window: ScanWindowSummary {
                start_height: self.window.effective_start_height,
                end_height: self.window.effective_end_height,
                chain_peak_height: self.window.chain_peak_height,
                incremental_from_checkpoint: self.request.incremental_from_checkpoint,
                auto_increment: self.request.auto_increment,
            },
            scan_stop_reason: self.stop_reason,
            coins: filtered,
        }
    }
}

use prepare::{ResolvedScanClient, ScanMetadata};

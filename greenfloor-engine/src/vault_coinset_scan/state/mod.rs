use std::collections::HashSet;

use crate::error::SignerResult;
use crate::vault_coinset_scan::cat_detect::{classify_coin_rows, CatDetectCaches};
use crate::vault_coinset_scan::checkpoint::{
    save_scan_checkpoint, CheckpointWriteMetadata, LoadCheckpointDiscardReason,
    LoadCheckpointResult, LoadedCheckpoint,
};
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

pub(super) struct ScanCheckpointContext {
    enabled: bool,
    resumed: bool,
    discard_reason: Option<LoadCheckpointDiscardReason>,
    start_nonce: u32,
}

impl ScanCheckpointContext {
    fn from_load(enabled: bool, load_result: LoadCheckpointResult) -> (Self, LoadedCheckpoint) {
        match load_result {
            LoadCheckpointResult::Loaded {
                checkpoint,
                start_nonce,
            } => {
                let payload = *checkpoint;
                let resumed = has_checkpoint_resume_data(&payload, start_nonce);
                (
                    Self {
                        enabled,
                        resumed,
                        discard_reason: None,
                        start_nonce,
                    },
                    payload,
                )
            }
            LoadCheckpointResult::Discarded(reason) => (
                Self {
                    enabled,
                    resumed: false,
                    discard_reason: Some(reason),
                    start_nonce: 0,
                },
                LoadedCheckpoint::empty(),
            ),
        }
    }

    fn summary(
        &self,
        file: Option<String>,
        save_interval: Option<u32>,
        cat_asset_cache_entries: usize,
        parent_lineage_cache_entries: usize,
        last_synced_height: Option<u64>,
    ) -> CheckpointSummary {
        CheckpointSummary {
            enabled: self.enabled,
            file,
            resumed: self.resumed,
            start_nonce: self.start_nonce,
            save_interval,
            cat_asset_cache_entries,
            parent_lineage_cache_entries,
            last_synced_height,
            discard_reason: self
                .discard_reason
                .map(|reason| reason.as_str().to_string()),
        }
    }
}

pub struct ScanState {
    request: ScanRequest,
    scanner: crate::coinset::DirectCoinsetScanClient,
    launcher_id: String,
    launcher_bytes: chia_protocol::Bytes32,
    effective_asset_type: crate::vault_coinset_scan::types::AssetTypeFilter,
    requested_cat_ids: HashSet<String>,
    asset_id_to_symbols: std::collections::BTreeMap<String, Vec<String>>,
    checkpoint_ctx: ScanCheckpointContext,
    checkpoint: LoadedCheckpoint,
    window: crate::vault_coinset_scan::window::ScanWindowPlan,
    stop_reason: ScanStopReason,
}

impl ScanState {
    /// Run.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
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
            &state.checkpoint.by_coin_id,
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
        let load_result = load_checkpoint_or_default(&request, &scanner.network, &launcher_id)?;
        let (checkpoint_ctx, checkpoint) =
            ScanCheckpointContext::from_load(checkpoint_enabled, load_result);

        let window = resolve_effective_window(
            &scanner,
            &request,
            checkpoint_ctx.enabled,
            checkpoint.last_synced_height,
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
            checkpoint_ctx,
            checkpoint,
            window,
            stop_reason: ScanStopReason::MaxNonceReached,
        })
    }

    async fn classify_rows(&mut self) -> SignerResult<()> {
        let mut detect_caches = CatDetectCaches::new(
            std::mem::take(&mut self.checkpoint.cat_asset_cache),
            std::mem::take(&mut self.checkpoint.parent_lineage_cache),
        );

        classify_coin_rows(
            &self.scanner,
            &mut self.checkpoint.by_coin_id,
            &self.checkpoint.nonce_to_p2,
            &self.asset_id_to_symbols,
            self.request.parent_lookup_batch_size,
            &mut detect_caches,
        )
        .await?;

        self.checkpoint.cat_asset_cache = detect_caches.cat_asset_cache;
        self.checkpoint.parent_lineage_cache = detect_caches.parent_lineage_cache;
        Ok(())
    }

    async fn verify_and_filter_names(&mut self) -> SignerResult<Option<NameVerification>> {
        if self.checkpoint.by_coin_id.is_empty() {
            return Ok(None);
        }
        let verified_coin_ids = self
            .scanner
            .existing_coin_names(
                &self
                    .checkpoint
                    .by_coin_id
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            .await?;
        Ok(apply_name_verification(
            &mut self.checkpoint.by_coin_id,
            &verified_coin_ids,
        ))
    }

    fn save_final_checkpoint(&mut self) -> SignerResult<()> {
        if !self.checkpoint_ctx.enabled {
            return Ok(());
        }
        self.write_checkpoint(self.checkpoint.max_nonce_scanned())
    }

    pub(super) fn write_checkpoint(&mut self, max_nonce_completed: u32) -> SignerResult<()> {
        self.checkpoint.last_synced_height = self.window.checkpoint_synced_height;
        save_scan_checkpoint(
            self.request
                .checkpoint_file
                .as_ref()
                .expect("checkpoint file"),
            &CheckpointWriteMetadata {
                network: &self.scanner.network,
                launcher_id: &self.launcher_id,
                include_spent: self.request.include_spent,
                max_nonce_completed,
                scan_start_height: self.window.effective_start_height,
                scan_end_height: self.window.effective_end_height,
            },
            &self.checkpoint,
        )
    }

    fn finish(
        self,
        filtered: Vec<CoinRow>,
        name_verification: Option<NameVerification>,
    ) -> ScanResult {
        let max_nonce_scanned = self.checkpoint.max_nonce_scanned();
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
            checkpoint: self.checkpoint_ctx.summary(
                self.request
                    .checkpoint_file
                    .map(|path| path.display().to_string()),
                if self.checkpoint_ctx.enabled {
                    Some(self.request.checkpoint_save_interval)
                } else {
                    None
                },
                self.checkpoint.cat_asset_cache.len(),
                self.checkpoint.parent_lineage_cache.len(),
                self.window.checkpoint_synced_height,
            ),
            scan_batches: ScanBatchConfig {
                nonce_batch_size: self.request.nonce_batch_size,
                empty_batch_stop_count: self.request.empty_batch_stop_count,
                parent_lookup_batch_size: self.request.parent_lookup_batch_size,
            },
            scan_window: ScanWindowSummary {
                start_height: self.window.effective_start_height,
                end_height: self.window.effective_end_height,
                chain_peak_height: self.window.chain_peak_height,
                incremental_from_checkpoint: self.request.checkpoint.incremental_from_checkpoint,
                auto_increment: self.request.checkpoint.auto_increment,
            },
            scan_stop_reason: self.stop_reason,
            coins: filtered,
        }
    }
}

fn has_checkpoint_resume_data(checkpoint: &LoadedCheckpoint, start_nonce: u32) -> bool {
    start_nonce > 0
        || !checkpoint.by_coin_id.is_empty()
        || !checkpoint.cat_asset_cache.is_empty()
        || !checkpoint.parent_lineage_cache.is_empty()
}

use prepare::{ResolvedScanClient, ScanMetadata};

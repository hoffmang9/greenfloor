use std::collections::{BTreeMap, HashMap, HashSet};

use chia_protocol::Bytes32;

use crate::coinset::{resolve_direct_client, DirectCoinsetScanClient};
use crate::config::build_cat_ticker_index;
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::vault::members::hex_to_bytes32;
use crate::vault_coinset_scan::cat_detect::{classify_coin_rows, CatDetectCaches};
use crate::vault_coinset_scan::checkpoint::{
    load_scan_checkpoint, save_scan_checkpoint, LoadedCheckpoint, SaveCheckpointParams,
};
use crate::vault_coinset_scan::metadata::resolve_requested_cat_ids;
use crate::vault_coinset_scan::request::ScanRequest;
use crate::vault_coinset_scan::result::{
    filter_rows, CheckpointSummary, NameVerification, ScanBatchConfig, ScanResult,
    ScanWindowSummary,
};
use crate::vault_coinset_scan::types::{AssetTypeFilter, CoinRow, ScanStopReason};
use crate::vault_coinset_scan::window::{resolve_scan_window, ScanWindowPlan};

mod nonce_scan;

pub struct ScanState {
    request: ScanRequest,
    scanner: DirectCoinsetScanClient,
    launcher_id: String,
    launcher_bytes: Bytes32,
    effective_asset_type: AssetTypeFilter,
    requested_cat_ids: HashSet<String>,
    asset_id_to_symbols: BTreeMap<String, Vec<String>>,
    checkpoint_enabled: bool,
    checkpoint_resumed: bool,
    checkpoint_discarded_mismatch: bool,
    checkpoint_start_nonce: u32,
    nonce_to_p2: HashMap<u32, String>,
    by_coin_id: HashMap<String, CoinRow>,
    cat_asset_cache: HashMap<String, String>,
    parent_lineage_cache:
        HashMap<String, crate::vault_coinset_scan::checkpoint::ParentLineageEntry>,
    window: ScanWindowPlan,
    stop_reason: ScanStopReason,
}

pub async fn run_vault_coinset_scan(request: ScanRequest) -> SignerResult<ScanResult> {
    ScanState::run(request).await
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
        let resolved = resolve_direct_client(&request.network, request.coinset_base_url.as_deref());
        let scanner =
            DirectCoinsetScanClient::new(resolved.network, Some(resolved.base_url.as_str()));

        let (ticker_to_asset_ids, asset_id_to_symbols) = build_cat_ticker_index(
            &request.cats_config,
            &request.markets_config,
            request.testnet_markets_config.as_deref(),
        )?;

        let requested_cat_ids_raw = request
            .requested_cat_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let (requested_cat_ids, unresolved_cat_tickers) = resolve_requested_cat_ids(
            &requested_cat_ids_raw,
            &request.requested_cat_tickers,
            &ticker_to_asset_ids,
        );
        if !unresolved_cat_tickers.is_empty() {
            return Err(SignerError::Other(format!(
                "unknown cat ticker(s): {}",
                unresolved_cat_tickers.join(", ")
            )));
        }

        let effective_asset_type =
            if !requested_cat_ids.is_empty() || !request.requested_cat_tickers.is_empty() {
                AssetTypeFilter::Cat
            } else {
                request.asset_type
            };

        let launcher_id = normalize_hex_id(&request.launcher_id);
        if launcher_id.is_empty() {
            return Err(SignerError::Other("launcher id is required".to_string()));
        }
        let launcher_bytes = hex_to_bytes32(&launcher_id)?;

        let checkpoint_enabled = request.checkpoint_file.is_some();
        let loaded = if checkpoint_enabled && !request.no_resume_checkpoint {
            let checkpoint_file = request.checkpoint_file.as_ref().expect("checkpoint file");
            load_scan_checkpoint(
                checkpoint_file,
                resolved.network,
                &launcher_id,
                request.include_spent,
            )?
        } else {
            LoadedCheckpoint {
                start_nonce: 0,
                nonce_to_p2: HashMap::new(),
                by_coin_id: HashMap::new(),
                cat_asset_cache: HashMap::new(),
                parent_lineage_cache: HashMap::new(),
                last_synced_height: None,
                discarded_mismatch: false,
            }
        };
        let checkpoint_discarded_mismatch = loaded.discarded_mismatch;
        let checkpoint_resumed = !loaded.discarded_mismatch
            && (loaded.start_nonce > 0
                || !loaded.by_coin_id.is_empty()
                || !loaded.cat_asset_cache.is_empty()
                || !loaded.parent_lineage_cache.is_empty());
        let LoadedCheckpoint {
            start_nonce: checkpoint_start_nonce,
            nonce_to_p2,
            by_coin_id,
            cat_asset_cache,
            parent_lineage_cache,
            last_synced_height: checkpoint_last_synced_height,
            ..
        } = loaded;

        if request.incremental_from_checkpoint && !checkpoint_enabled {
            return Err(SignerError::Other(
                "--incremental-from-checkpoint requires --checkpoint-file".to_string(),
            ));
        }

        let chain_peak_height =
            if request.incremental_from_checkpoint || request.end_height.is_none() {
                scanner.chain_peak_height().await?
            } else {
                None
            };

        let window = resolve_scan_window(
            request.start_height,
            request.end_height,
            request.incremental_from_checkpoint,
            checkpoint_last_synced_height,
            chain_peak_height,
        )?;

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
        let pre_verify_count = self.by_coin_id.len();
        if pre_verify_count == 0 {
            return Ok(None);
        }
        let verified_coin_ids = self
            .scanner
            .existing_coin_names(&self.by_coin_id.keys().cloned().collect::<Vec<_>>())
            .await?;
        let verified_set: HashSet<String> = verified_coin_ids.into_iter().collect();
        self.by_coin_id
            .retain(|coin_id, _| verified_set.contains(coin_id));
        let dropped_unverified_count = pre_verify_count.saturating_sub(self.by_coin_id.len());
        Ok(Some(NameVerification {
            applied: true,
            pre_verify_count,
            verified_count: Some(self.by_coin_id.len()),
            dropped_unverified_count,
        }))
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

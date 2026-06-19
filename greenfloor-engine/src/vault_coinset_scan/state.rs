use std::collections::{BTreeMap, HashMap, HashSet};

use chia_protocol::Bytes32;
use serde_json::Value;

use crate::coinset::{
    chunk_values, coin_id_from_record, resolve_direct_client, to_coinset_hex, u64_from_value,
    DirectCoinsetScanClient,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::vault::members::{
    hex_to_bytes32, singleton_member_hash, tree_hash_to_hex, MemberConfig,
};
use crate::vault_coinset_scan::cat_detect::{classify_coin_rows, CatDetectCaches};
use crate::vault_coinset_scan::checkpoint::{
    load_scan_checkpoint, save_scan_checkpoint, LoadedCheckpoint, ParentLineageEntry,
    SaveCheckpointParams,
};
use crate::vault_coinset_scan::metadata::{load_cat_metadata_indexes, resolve_requested_cat_ids};
use crate::vault_coinset_scan::request::ScanRequest;
use crate::vault_coinset_scan::result::{
    filter_rows, NameVerification, ScanResult, ScanResultParams,
};
use crate::vault_coinset_scan::types::{AssetTypeFilter, CoinKind, CoinRow, ScanStopReason};
use crate::vault_coinset_scan::window::{resolve_scan_window, ScanWindowPlan};

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
    checkpoint_start_nonce: u32,
    nonce_to_p2: HashMap<u32, String>,
    by_coin_id: HashMap<String, CoinRow>,
    cat_asset_cache: HashMap<String, String>,
    parent_lineage_cache: HashMap<String, ParentLineageEntry>,
    window: ScanWindowPlan,
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
        state.prefetch_parent_records().await?;
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

        let (ticker_to_asset_ids, asset_id_to_symbols) = load_cat_metadata_indexes(
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
        let mut checkpoint_resumed = false;
        let LoadedCheckpoint {
            start_nonce: checkpoint_start_nonce,
            nonce_to_p2,
            by_coin_id,
            cat_asset_cache,
            parent_lineage_cache,
            last_synced_height: checkpoint_last_synced_height,
        } = if checkpoint_enabled && !request.no_resume_checkpoint {
            let checkpoint_file = request.checkpoint_file.as_ref().expect("checkpoint file");
            let loaded = load_scan_checkpoint(
                checkpoint_file,
                resolved.network,
                &launcher_id,
                request.include_spent,
            );
            checkpoint_resumed = loaded.start_nonce > 0
                || !loaded.by_coin_id.is_empty()
                || !loaded.cat_asset_cache.is_empty()
                || !loaded.parent_lineage_cache.is_empty();
            loaded
        } else {
            LoadedCheckpoint {
                start_nonce: 0,
                nonce_to_p2: HashMap::new(),
                by_coin_id: HashMap::new(),
                cat_asset_cache: HashMap::new(),
                parent_lineage_cache: HashMap::new(),
                last_synced_height: None,
            }
        };

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
            checkpoint_start_nonce,
            nonce_to_p2,
            by_coin_id,
            cat_asset_cache,
            parent_lineage_cache,
            window,
            stop_reason: ScanStopReason::MaxNonceReached,
        })
    }

    async fn scan_nonces(&mut self) -> SignerResult<()> {
        let max_nonce_target = self.request.max_nonce;
        let nonce_batch_size = self.request.nonce_batch_size;
        let empty_batch_stop_count = self.request.empty_batch_stop_count;
        let checkpoint_save_interval = self.request.checkpoint_save_interval;
        let mut scanned_since_resume = 0u32;
        let mut empty_batch_count = 0u32;

        for batch_start in
            (self.checkpoint_start_nonce..=max_nonce_target).step_by(nonce_batch_size as usize)
        {
            let batch_end = batch_start
                .saturating_add(nonce_batch_size.saturating_sub(1))
                .min(max_nonce_target);
            let batch_nonces: Vec<u32> = (batch_start..=batch_end).collect();
            let batch_nonce_p2 = self.build_batch_nonce_p2(&batch_nonces)?;
            let p2_hashes = Self::coinset_p2_hashes(&batch_nonce_p2);

            let by_puzzle = self
                .scanner
                .by_puzzle_hashes(
                    &p2_hashes,
                    self.request.include_spent,
                    self.window.effective_start_height,
                    self.window.effective_end_height,
                )
                .await?;
            let by_hint = self
                .scanner
                .by_hints(
                    &p2_hashes,
                    self.request.include_spent,
                    self.window.effective_start_height,
                    self.window.effective_end_height,
                )
                .await?;
            let batch_has_any = !by_puzzle.is_empty() || !by_hint.is_empty();
            if batch_end > 0 && !batch_has_any {
                empty_batch_count = empty_batch_count.saturating_add(1);
            } else {
                empty_batch_count = 0;
            }
            if empty_batch_count >= empty_batch_stop_count {
                self.stop_reason = ScanStopReason::EmptyNonceBatches;
                if self.checkpoint_enabled {
                    self.write_checkpoint(batch_end)?;
                }
                break;
            }

            ingest_records(
                &mut self.by_coin_id,
                &batch_nonce_p2,
                "puzzle_hash",
                &by_puzzle,
            );
            ingest_records(&mut self.by_coin_id, &batch_nonce_p2, "hint", &by_hint);

            scanned_since_resume = scanned_since_resume
                .saturating_add(u32::try_from(batch_nonces.len()).unwrap_or(u32::MAX));
            if self.checkpoint_enabled
                && (scanned_since_resume.is_multiple_of(checkpoint_save_interval)
                    || batch_end >= max_nonce_target)
            {
                self.write_checkpoint(batch_end)?;
            }
        }
        Ok(())
    }

    fn build_batch_nonce_p2(&mut self, batch_nonces: &[u32]) -> SignerResult<HashMap<u32, String>> {
        let mut batch_nonce_p2 = HashMap::new();
        for nonce in batch_nonces {
            let config = MemberConfig::default()
                .with_top_level(true)
                .with_nonce(*nonce);
            let p2_hash =
                tree_hash_to_hex(singleton_member_hash(&config, self.launcher_bytes, false)?);
            let normalized = normalize_hex_id(&p2_hash);
            if !normalized.is_empty() {
                batch_nonce_p2.insert(*nonce, normalized.clone());
                self.nonce_to_p2.insert(*nonce, normalized);
            }
        }
        Ok(batch_nonce_p2)
    }

    fn coinset_p2_hashes(batch_nonce_p2: &HashMap<u32, String>) -> Vec<String> {
        batch_nonce_p2
            .values()
            .filter_map(|value| {
                hex_to_bytes32(value)
                    .ok()
                    .map(|bytes| to_coinset_hex(bytes.as_ref()))
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
    }

    async fn prefetch_parent_records(&mut self) -> SignerResult<HashMap<String, Value>> {
        let unresolved_parent_ids: Vec<String> = self
            .by_coin_id
            .values()
            .filter_map(|row| {
                if row.parent_coin_info.is_empty() {
                    None
                } else {
                    Some(row.parent_coin_info.clone())
                }
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let mut parent_record_cache: HashMap<String, Value> = HashMap::new();
        for parent_batch in chunk_values(
            &unresolved_parent_ids,
            self.request.parent_lookup_batch_size as usize,
        ) {
            let parent_records = self
                .scanner
                .by_names(
                    &parent_batch
                        .iter()
                        .filter_map(|parent_id| {
                            hex_to_bytes32(parent_id)
                                .ok()
                                .map(|bytes| to_coinset_hex(bytes.as_ref()))
                        })
                        .collect::<Vec<_>>(),
                    true,
                    None,
                    None,
                )
                .await?;
            for parent in parent_records {
                let parent_id = coin_id_from_record(&parent);
                if !parent_id.is_empty() {
                    parent_record_cache.insert(parent_id, parent);
                }
            }
        }
        Ok(parent_record_cache)
    }

    async fn classify_rows(&mut self) -> SignerResult<()> {
        let parent_record_cache = self.prefetch_parent_records().await?;
        let mut detect_caches = CatDetectCaches::new(
            std::mem::take(&mut self.cat_asset_cache),
            std::mem::take(&mut self.parent_lineage_cache),
        );
        detect_caches.parent_record_cache = parent_record_cache
            .into_iter()
            .map(|(key, value)| (key, Some(value)))
            .collect();

        classify_coin_rows(
            &self.scanner,
            &mut self.by_coin_id,
            &self.nonce_to_p2,
            &self.asset_id_to_symbols,
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

    fn write_checkpoint(&self, max_nonce_completed: u32) -> SignerResult<()> {
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
        ScanResult::build(ScanResultParams {
            scanner: &self.scanner,
            request: &self.request,
            launcher_id: &self.launcher_id,
            effective_asset_type: self.effective_asset_type,
            requested_cat_ids: &self.requested_cat_ids,
            nonce_to_p2: &self.nonce_to_p2,
            cat_asset_cache: &self.cat_asset_cache,
            parent_lineage_cache: &self.parent_lineage_cache,
            checkpoint_enabled: self.checkpoint_enabled,
            checkpoint_resumed: self.checkpoint_resumed,
            checkpoint_start_nonce: self.checkpoint_start_nonce,
            window: &self.window,
            stop_reason: self.stop_reason,
            filtered,
            name_verification,
        })
    }
}

fn ingest_records(
    by_coin_id: &mut HashMap<String, CoinRow>,
    batch_nonce_p2: &HashMap<u32, String>,
    source: &str,
    records: &[Value],
) {
    for record in records {
        let coin_id = coin_id_from_record(record);
        if coin_id.is_empty() {
            continue;
        }
        let coin = record.get("coin").and_then(Value::as_object);
        let row = by_coin_id
            .entry(coin_id.clone())
            .or_insert_with(|| CoinRow {
                coin_id: coin_id.clone(),
                puzzle_hash: coin
                    .and_then(|value| value.get("puzzle_hash"))
                    .and_then(Value::as_str)
                    .map(normalize_hex_id)
                    .unwrap_or_default(),
                parent_coin_info: coin
                    .and_then(|value| value.get("parent_coin_info"))
                    .and_then(Value::as_str)
                    .map(normalize_hex_id)
                    .unwrap_or_default(),
                amount: u64_from_value(coin.and_then(|value| value.get("amount")), 0),
                confirmed_block_index: u64_from_value(record.get("confirmed_block_index"), 0),
                spent_block_index: u64_from_value(record.get("spent_block_index"), 0),
                discovered_nonces: Vec::new(),
                discovered_by_puzzle_hash: false,
                discovered_by_hint: false,
                kind: CoinKind::Unknown,
                cat_asset_id: None,
                cat_symbols: Vec::new(),
            });
        for (nonce, batch_p2) in batch_nonce_p2 {
            if row.puzzle_hash == *batch_p2 && !row.discovered_nonces.contains(nonce) {
                row.discovered_nonces.push(*nonce);
            }
        }
        row.discovered_nonces.sort_unstable();
        if source == "puzzle_hash" {
            row.discovered_by_puzzle_hash = true;
        }
        if source == "hint" {
            row.discovered_by_hint = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_records_marks_discovery_sources() {
        let puzzle = "a".repeat(64);
        let parent = "c".repeat(64);
        let coin_id = "b".repeat(64);
        let record = serde_json::json!({
            "coin": {
                "name": coin_id,
                "parent_coin_info": parent,
                "puzzle_hash": puzzle,
                "amount": 1000,
            },
            "confirmed_block_index": 1,
            "spent_block_index": 0,
        });
        let mut by_coin_id = HashMap::new();
        let mut batch_nonce_p2 = HashMap::new();
        batch_nonce_p2.insert(0, puzzle.clone());
        ingest_records(&mut by_coin_id, &batch_nonce_p2, "puzzle_hash", &[record]);
        assert_eq!(by_coin_id.len(), 1);
        let row = by_coin_id.values().next().expect("row");
        assert!(row.discovered_by_puzzle_hash);
        assert_eq!(row.discovered_nonces, vec![0]);
    }
}

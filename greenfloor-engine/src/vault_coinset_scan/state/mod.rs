use std::collections::{HashMap, HashSet};

use crate::error::SignerResult;
use crate::vault_coinset_scan::cat_detect::{classify_coin_rows, CatDetectCaches};
use crate::vault_coinset_scan::checkpoint::save_scan_checkpoint;
use crate::vault_coinset_scan::checkpoint::{ParentLineageEntry, SaveCheckpointParams};
use crate::vault_coinset_scan::request::ScanRequest;
use crate::vault_coinset_scan::result::{
    filter_rows, CheckpointSummary, NameVerification, ScanBatchConfig, ScanResult,
    ScanWindowSummary,
};
use crate::vault_coinset_scan::types::{CoinRow, ScanStopReason};

mod nonce_scan;
mod prepare;

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

use prepare::{ResolvedScanClient, ScanMetadata};

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chia_protocol::CoinSpend;
    use mockito::Matcher;
    use serde_json::json;

    use crate::coinset::{coin_id_from_record, to_coinset_hex};
    use crate::hex::normalize_hex_id;
    use crate::test_support::simulator::harness::SimulatorVaultHarness;
    use crate::vault::members::hex_to_bytes32;
    use crate::vault_coinset_scan::request::ScanRequest;
    use crate::vault_coinset_scan::types::{AssetTypeFilter, CoinKind};

    use super::run_vault_coinset_scan;

    struct CatScanFixtures {
        launcher_id: String,
        cat_coin_record: serde_json::Value,
        cat_coin_name: String,
        parent_coin_record: serde_json::Value,
        parent_coin_name: String,
        parent_spent_height: u64,
        puzzle_and_solution: serde_json::Value,
        cat_asset_id: String,
    }

    fn build_cat_scan_fixtures() -> CatScanFixtures {
        let mut harness = SimulatorVaultHarness::new();
        harness.mint_vault();
        let cat = harness.fund_vault_cat(5_000);
        let launcher_id = hex::encode(harness.chain.launcher_id);
        let cat_coin_id = hex::encode(cat.coin.coin_id());
        let parent_coin_id = cat.coin.parent_coin_info;
        let sim = harness.chain.sim.lock().expect("sim lock");
        let parent_spend = sim
            .coin_spend(parent_coin_id)
            .expect("parent spend for cat");
        let parent_state = sim.coin_state(parent_coin_id).expect("parent coin state");
        drop(sim);

        let parent_spent_height = u64::from(parent_state.spent_height.unwrap_or(1));
        let parent_confirmed_height = u64::from(parent_state.created_height.unwrap_or(1));
        let parent_coin_record = json!({
            "coin": {
                "name": hex::encode(parent_coin_id),
                "parent_coin_info": hex::encode(parent_state.coin.parent_coin_info),
                "puzzle_hash": hex::encode(parent_state.coin.puzzle_hash),
                "amount": parent_state.coin.amount,
            },
            "confirmed_block_index": parent_confirmed_height,
            "spent_block_index": parent_spent_height,
        });
        let cat_coin_record = json!({
            "coin": {
                "name": cat_coin_id.clone(),
                "parent_coin_info": hex::encode(cat.coin.parent_coin_info),
                "puzzle_hash": hex::encode(cat.coin.puzzle_hash),
                "amount": cat.coin.amount,
            },
            "confirmed_block_index": 12,
            "spent_block_index": 0,
        });
        let parent_spend = CoinSpend {
            coin: parent_spend.coin,
            puzzle_reveal: parent_spend.puzzle_reveal.clone(),
            solution: parent_spend.solution.clone(),
        };
        let puzzle_and_solution = json!({
            "puzzle_reveal": format!("0x{}", hex::encode(parent_spend.puzzle_reveal.as_ref())),
            "solution": format!("0x{}", hex::encode(parent_spend.solution.as_ref())),
        });

        CatScanFixtures {
            launcher_id: normalize_hex_id(&launcher_id),
            cat_coin_name: to_coinset_hex(hex_to_bytes32(&cat_coin_id).expect("cat id").as_ref()),
            parent_coin_name: to_coinset_hex(
                hex_to_bytes32(&hex::encode(parent_coin_id))
                    .expect("parent id")
                    .as_ref(),
            ),
            cat_coin_record,
            parent_coin_record,
            parent_spent_height,
            puzzle_and_solution,
            cat_asset_id: normalize_hex_id(&hex::encode(cat.info.asset_id)),
        }
    }

    #[tokio::test]
    async fn run_vault_coinset_scan_discovers_cat_via_parent_spend() {
        let fixtures = build_cat_scan_fixtures();
        let coin_records_body = json!({
            "success": true,
            "coin_records": [fixtures.cat_coin_record.clone()],
        })
        .to_string();
        let parent_records_body = json!({
            "success": true,
            "coin_records": [fixtures.parent_coin_record.clone()],
        })
        .to_string();
        let puzzle_solution_body = json!({
            "success": true,
            "coin_solution": fixtures.puzzle_and_solution.clone(),
        })
        .to_string();

        let mut server = mockito::Server::new_async().await;
        let _puzzle_mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hashes")
            .with_status(200)
            .with_body(coin_records_body.clone())
            .create_async()
            .await;
        let _hint_mock = server
            .mock("POST", "/get_coin_records_by_hints")
            .with_status(200)
            .with_body(r#"{"success":true,"coin_records":[]}"#)
            .create_async()
            .await;
        let _parent_mock = server
            .mock("POST", "/get_coin_records_by_names")
            .match_body(Matcher::PartialJson(json!({
                "names": [fixtures.parent_coin_name.clone()],
                "include_spent_coins": true,
            })))
            .with_status(200)
            .with_body(parent_records_body)
            .create_async()
            .await;
        let _verify_mock = server
            .mock("POST", "/get_coin_records_by_names")
            .match_body(Matcher::PartialJson(json!({
                "names": [fixtures.cat_coin_name.clone()],
                "include_spent_coins": true,
            })))
            .with_status(200)
            .with_body(coin_records_body)
            .create_async()
            .await;
        let _solution_mock = server
            .mock("POST", "/get_puzzle_and_solution")
            .match_body(Matcher::PartialJson(json!({
                "coin_id": fixtures.parent_coin_name.clone(),
                "height": fixtures.parent_spent_height,
            })))
            .with_status(200)
            .with_body(puzzle_solution_body)
            .create_async()
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let request = ScanRequest {
            network: "mainnet".to_string(),
            coinset_base_url: Some(server.url()),
            launcher_id: fixtures.launcher_id.clone(),
            max_nonce: 0,
            include_spent: false,
            asset_type: AssetTypeFilter::All,
            requested_cat_ids: HashSet::new(),
            requested_cat_tickers: Vec::new(),
            checkpoint_file: None,
            checkpoint_save_interval: 1,
            no_resume_checkpoint: false,
            nonce_batch_size: 32,
            empty_batch_stop_count: 1,
            parent_lookup_batch_size: 64,
            start_height: None,
            end_height: Some(100),
            incremental_from_checkpoint: false,
            auto_increment: false,
            cats_config: dir.path().join("missing-cats.yaml"),
            markets_config: dir.path().join("missing-markets.yaml"),
            testnet_markets_config: None,
            cache_clear: None,
        };

        let result = run_vault_coinset_scan(request)
            .await
            .expect("scan should classify cat");
        assert_eq!(result.count, 1);
        assert_eq!(result.coins.len(), 1);
        assert_eq!(result.coins[0].kind, CoinKind::Cat);
        assert_eq!(
            result.coins[0].cat_asset_id.as_deref(),
            Some(fixtures.cat_asset_id.as_str())
        );
        assert_eq!(
            result.coins[0].coin_id,
            coin_id_from_record(&fixtures.cat_coin_record)
        );
    }
}

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::coinset::{coin_id_from_record, to_coinset_hex, u64_from_value};
use crate::error::SignerResult;
use crate::hex::{hex_to_bytes32, normalize_hex_id};
use crate::vault::members::nonce_member_puzzle_hash_hex;
use crate::vault_coinset_scan::cat_outer::cat_outer_coinset_hex;
use crate::vault_coinset_scan::types::{
    AssetTypeFilter, CoinKind, CoinRow, DiscoverySource, ScanStopReason,
};

use super::ScanState;

impl ScanState {
    pub(super) async fn scan_nonces(&mut self) -> SignerResult<()> {
        let nonce_batch_size = self.request.nonce_batch_size;
        let empty_batch_stop_count = self.request.empty_batch_stop_count;
        let checkpoint_save_interval = self.request.checkpoint_save_interval;
        let mut scanned_since_resume = 0u32;
        let mut empty_batch_count = 0u32;
        let cat_scoped = self.is_cat_scoped_scan();
        let stop_after_empty_batches = match self.request.discovery.empty_batch_stop() {
            crate::vault_coinset_scan::EmptyBatchStop::Always => true,
            crate::vault_coinset_scan::EmptyBatchStop::WhenUnfiltered => {
                self.window.effective_start_height.is_none()
            }
        };

        self.scan_extra_hint_hashes().await?;

        // Hint-only plans skip the member walk. `Nonces` / `HintsThenNonces` with
        // `max_nonce: 0` still scan nonce 0.
        let Some(max_nonce_target) = self.request.discovery.max_nonce() else {
            self.stop_reason = ScanStopReason::MaxNonceReached;
            return Ok(());
        };

        for batch_start in
            (self.checkpoint_ctx.start_nonce..=max_nonce_target).step_by(nonce_batch_size as usize)
        {
            let batch_end = batch_start
                .saturating_add(nonce_batch_size.saturating_sub(1))
                .min(max_nonce_target);
            let batch_nonces: Vec<u32> = (batch_start..=batch_end).collect();
            let batch_nonce_p2 = self.build_batch_nonce_p2(&batch_nonces)?;
            let p2_hashes = coinset_p2_hashes(&batch_nonce_p2);

            let (by_puzzle, by_hint) = self.fetch_nonce_batch(&p2_hashes, cat_scoped).await?;

            let batch_has_any = !by_puzzle.is_empty() || !by_hint.is_empty();
            tracing::debug!(
                batch_start,
                batch_end,
                puzzle_hits = by_puzzle.len(),
                hint_hits = by_hint.len(),
                discovered_total = self.checkpoint.by_coin_id.len(),
                cat_scoped,
                "vault coinset scan nonce batch"
            );
            if batch_end > 0 && !batch_has_any {
                empty_batch_count = empty_batch_count.saturating_add(1);
            } else {
                empty_batch_count = 0;
            }
            if should_stop_after_empty_batch(
                batch_end,
                empty_batch_count,
                empty_batch_stop_count,
                stop_after_empty_batches,
            ) {
                self.stop_reason = ScanStopReason::EmptyNonceBatches;
                if self.checkpoint_ctx.enabled {
                    self.write_checkpoint(batch_end)?;
                }
                break;
            }

            ingest_records(
                &mut self.checkpoint.by_coin_id,
                &batch_nonce_p2,
                DiscoverySource::PuzzleHash,
                &by_puzzle,
            );
            ingest_records(
                &mut self.checkpoint.by_coin_id,
                &batch_nonce_p2,
                DiscoverySource::Hint,
                &by_hint,
            );

            scanned_since_resume = scanned_since_resume
                .saturating_add(u32::try_from(batch_nonces.len()).unwrap_or(u32::MAX));
            if self.checkpoint_ctx.enabled
                && (scanned_since_resume.is_multiple_of(checkpoint_save_interval)
                    || batch_end >= max_nonce_target)
            {
                self.write_checkpoint(batch_end)?;
            }
        }
        Ok(())
    }

    fn is_cat_scoped_scan(&self) -> bool {
        matches!(self.effective_asset_type, AssetTypeFilter::Cat)
            || !self.requested_cat_ids.is_empty()
    }

    /// CAT coins live on outer CAT puzzle hashes; vault discovery is via `CREATE_COIN` hints.
    /// Skipping `by_puzzle_hashes` avoids pulling the full historical XCH set when
    /// `include_spent` is true (vault-asset-trace).
    async fn fetch_nonce_batch(
        &self,
        p2_hashes: &[String],
        cat_scoped: bool,
    ) -> SignerResult<(Vec<Value>, Vec<Value>)> {
        if cat_scoped {
            let by_hint = self
                .scanner
                .by_hints(
                    p2_hashes,
                    self.request.include_spent,
                    self.window.effective_start_height,
                    self.window.effective_end_height,
                )
                .await?;
            return Ok((Vec::new(), by_hint));
        }
        let (by_puzzle, by_hint) = tokio::join!(
            self.scanner.by_puzzle_hashes(
                p2_hashes,
                self.request.include_spent,
                self.window.effective_start_height,
                self.window.effective_end_height,
            ),
            self.scanner.by_hints(
                p2_hashes,
                self.request.include_spent,
                self.window.effective_start_height,
                self.window.effective_end_height,
            ),
        );
        Ok((by_puzzle?, by_hint?))
    }

    async fn scan_extra_hint_hashes(&mut self) -> SignerResult<()> {
        let extra_p2: Vec<String> = self
            .request
            .discovery
            .hint_puzzle_hashes()
            .iter()
            .map(|value| normalize_hex_id(value))
            .filter(|value| !value.is_empty())
            .collect();
        if extra_p2.is_empty() {
            return Ok(());
        }

        let empty_nonce_p2 = HashMap::new();
        if !self.requested_cat_ids.is_empty() {
            // Scoped CAT discovery: query receive CAT outer puzzle hashes only.
            // by_hints(receive_p2) with include_spent returns every asset ever sent to
            // the vault and makes classify unbounded for vault-asset-trace.
            let mut outer_hashes = Vec::new();
            for p2_hex in &extra_p2 {
                for asset_id in &self.requested_cat_ids {
                    if let Some(outer) = cat_outer_coinset_hex(asset_id, p2_hex) {
                        outer_hashes.push(outer);
                    }
                }
            }
            if outer_hashes.is_empty() {
                return Ok(());
            }
            let by_puzzle = self
                .scanner
                .by_puzzle_hashes(
                    &outer_hashes,
                    self.request.include_spent,
                    self.window.effective_start_height,
                    self.window.effective_end_height,
                )
                .await?;
            tracing::debug!(
                outer_hashes = outer_hashes.len(),
                puzzle_hits = by_puzzle.len(),
                "vault coinset scan CAT receive outer puzzles"
            );
            ingest_records(
                &mut self.checkpoint.by_coin_id,
                &empty_nonce_p2,
                DiscoverySource::PuzzleHash,
                &by_puzzle,
            );
            return Ok(());
        }

        let extra = extra_p2
            .iter()
            .filter_map(|value| {
                hex_to_bytes32(value)
                    .ok()
                    .map(|bytes| to_coinset_hex(bytes.as_ref()))
            })
            .collect::<Vec<_>>();
        let by_hint = self
            .scanner
            .by_hints(
                &extra,
                self.request.include_spent,
                self.window.effective_start_height,
                self.window.effective_end_height,
            )
            .await?;
        tracing::debug!(
            hint_hashes = extra.len(),
            hint_hits = by_hint.len(),
            "vault coinset scan extra receive hints"
        );
        ingest_records(
            &mut self.checkpoint.by_coin_id,
            &empty_nonce_p2,
            DiscoverySource::Hint,
            &by_hint,
        );
        Ok(())
    }

    fn build_batch_nonce_p2(&mut self, batch_nonces: &[u32]) -> SignerResult<HashMap<u32, String>> {
        let mut batch_nonce_p2 = HashMap::new();
        for nonce in batch_nonces {
            let p2_hash = nonce_member_puzzle_hash_hex(self.launcher_bytes, *nonce)?;
            let normalized = normalize_hex_id(&p2_hash);
            if !normalized.is_empty() {
                batch_nonce_p2.insert(*nonce, normalized.clone());
                self.checkpoint.nonce_to_p2.insert(*nonce, normalized);
            }
        }
        Ok(batch_nonce_p2)
    }
}

fn should_stop_after_empty_batch(
    batch_end: u32,
    empty_batch_count: u32,
    empty_batch_stop_count: u32,
    stop_after_empty_batches: bool,
) -> bool {
    stop_after_empty_batches && batch_end > 0 && empty_batch_count >= empty_batch_stop_count
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

pub(super) fn ingest_records(
    by_coin_id: &mut HashMap<String, CoinRow>,
    batch_nonce_p2: &HashMap<u32, String>,
    source: DiscoverySource,
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
        match source {
            DiscoverySource::PuzzleHash => row.discovered_by_puzzle_hash = true,
            DiscoverySource::Hint => row.discovered_by_hint = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coinset::test_support::mock_get_coin_records_by_puzzle_hash_body;
    use crate::vault_coinset_scan::request::{MemberDiscovery, ScanCheckpointControl, ScanRequest};
    use crate::vault_coinset_scan::types::AssetTypeFilter;
    use chia_protocol::{Bytes32, Coin};
    use mockito::Matcher;
    use std::path::PathBuf;

    fn scan_request(base_url: String, launcher_id: &str, start_height: Option<u64>) -> ScanRequest {
        ScanRequest {
            network: "mainnet".to_string(),
            coinset_base_url: Some(base_url),
            launcher_id: launcher_id.to_string(),
            discovery: MemberDiscovery::nonces(63),
            include_spent: false,
            asset_type: AssetTypeFilter::Xch,
            requested_cat_ids: HashSet::new(),
            requested_cat_tickers: Vec::new(),
            checkpoint_file: None,
            checkpoint_save_interval: 1,
            checkpoint: ScanCheckpointControl {
                no_resume_checkpoint: true,
                incremental_from_checkpoint: false,
                auto_increment: false,
            },
            nonce_batch_size: 32,
            empty_batch_stop_count: 1,
            parent_lookup_batch_size: 64,
            start_height,
            end_height: Some(200),
            cats_config: PathBuf::new(),
            markets_config: PathBuf::new(),
            testnet_markets_config: None,
            cache_clear: None,
        }
    }

    #[tokio::test]
    async fn height_filtered_scan_reaches_later_nonce_after_empty_batch() {
        let mut server = mockito::Server::new_async().await;
        let launcher_id = "11".repeat(32);
        let launcher_bytes = hex_to_bytes32(&launcher_id).expect("launcher id");
        let first_p2 =
            nonce_member_puzzle_hash_hex(launcher_bytes, 0).expect("first nonce puzzle hash");
        let later_p2 =
            nonce_member_puzzle_hash_hex(launcher_bytes, 32).expect("later nonce puzzle hash");
        let later_coin = Coin::new(
            Bytes32::new([0x22; 32]),
            hex_to_bytes32(&later_p2).expect("later puzzle hash"),
            123,
        );

        for endpoint in [
            "/get_coin_records_by_puzzle_hashes",
            "/get_coin_records_by_hints",
        ] {
            server
                .mock("POST", endpoint)
                .match_body(Matcher::Regex(first_p2.clone()))
                .with_status(200)
                .with_body(r#"{"success":true,"coin_records":[]}"#)
                .create();
            server
                .mock("POST", endpoint)
                .match_body(Matcher::Regex(later_p2.clone()))
                .with_status(200)
                .with_body(mock_get_coin_records_by_puzzle_hash_body(&[later_coin]))
                .create();
        }

        let mut state = ScanState::prepare(scan_request(server.url(), &launcher_id, Some(100)))
            .await
            .expect("prepare scan");
        state.scan_nonces().await.expect("scan nonces");

        assert_eq!(state.stop_reason, ScanStopReason::MaxNonceReached);
        assert!(state
            .checkpoint
            .by_coin_id
            .contains_key(&normalize_hex_id(&hex::encode(later_coin.coin_id()))));
    }

    #[test]
    fn height_filtered_scan_does_not_stop_on_empty_nonce_batch() {
        assert!(!should_stop_after_empty_batch(32, 1, 1, false));
    }

    #[test]
    fn empty_batch_stop_honors_flag() {
        assert!(should_stop_after_empty_batch(32, 1, 1, true));
        assert!(!should_stop_after_empty_batch(32, 1, 1, false));
        assert!(!should_stop_after_empty_batch(0, 1, 1, true));
    }

    #[tokio::test]
    async fn hint_only_discovery_skips_member_nonce_walk() {
        use crate::vault_coinset_scan::cat_outer::cat_outer_coinset_hex;

        let mut server = mockito::Server::new_async().await;
        let launcher_id = "11".repeat(32);
        let asset_id = "aa".repeat(32);
        let receive_p2 = "bb".repeat(32);
        let outer = cat_outer_coinset_hex(&asset_id, &receive_p2).expect("outer");
        let outer_coin = Coin::new(
            Bytes32::new([0x33; 32]),
            hex_to_bytes32(&normalize_hex_id(&outer)).expect("outer bytes"),
            1000,
        );

        // Hint-only CAT path queries outer puzzle hashes, never member nonce endpoints.
        let outer_mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hashes")
            .match_body(Matcher::Regex(normalize_hex_id(&outer)))
            .with_status(200)
            .with_body(mock_get_coin_records_by_puzzle_hash_body(&[outer_coin]))
            .expect(1)
            .create();
        let hints_mock = server
            .mock("POST", "/get_coin_records_by_hints")
            .with_status(200)
            .with_body(r#"{"success":true,"coin_records":[]}"#)
            .expect(0)
            .create();

        let mut request = scan_request(server.url(), &launcher_id, Some(100));
        request.asset_type = AssetTypeFilter::Cat;
        request.requested_cat_ids = HashSet::from([asset_id.clone()]);
        request.include_spent = true;
        request.discovery = MemberDiscovery::Hints {
            puzzle_hashes: vec![receive_p2],
        };

        let mut state = ScanState::prepare(request).await.expect("prepare scan");
        state.scan_nonces().await.expect("scan nonces");

        assert_eq!(state.stop_reason, ScanStopReason::MaxNonceReached);
        assert!(state
            .checkpoint
            .by_coin_id
            .contains_key(&normalize_hex_id(&hex::encode(outer_coin.coin_id()))));
        outer_mock.assert();
        hints_mock.assert();
    }

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
        ingest_records(
            &mut by_coin_id,
            &batch_nonce_p2,
            DiscoverySource::PuzzleHash,
            &[record],
        );
        assert_eq!(by_coin_id.len(), 1);
        let row = by_coin_id.values().next().expect("row");
        assert!(row.discovered_by_puzzle_hash);
        assert_eq!(row.discovered_nonces, vec![0]);
    }
}

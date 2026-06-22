use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::coinset::{coin_id_from_record, to_coinset_hex, u64_from_value};
use crate::error::SignerResult;
use crate::hex::normalize_hex_id;
use crate::vault::members::{
    hex_to_bytes32, singleton_member_hash, tree_hash_to_hex, MemberConfig,
};
use crate::vault_coinset_scan::types::{CoinKind, CoinRow, DiscoverySource, ScanStopReason};

use super::ScanState;

impl ScanState {
    pub(super) async fn scan_nonces(&mut self) -> SignerResult<()> {
        let max_nonce_target = self.request.max_nonce;
        let nonce_batch_size = self.request.nonce_batch_size;
        let empty_batch_stop_count = self.request.empty_batch_stop_count;
        let checkpoint_save_interval = self.request.checkpoint_save_interval;
        let mut scanned_since_resume = 0u32;
        let mut empty_batch_count = 0u32;

        for batch_start in
            (self.checkpoint_ctx.start_nonce..=max_nonce_target).step_by(nonce_batch_size as usize)
        {
            let batch_end = batch_start
                .saturating_add(nonce_batch_size.saturating_sub(1))
                .min(max_nonce_target);
            let batch_nonces: Vec<u32> = (batch_start..=batch_end).collect();
            let batch_nonce_p2 = self.build_batch_nonce_p2(&batch_nonces)?;
            let p2_hashes = coinset_p2_hashes(&batch_nonce_p2);

            let (by_puzzle, by_hint) = tokio::join!(
                self.scanner.by_puzzle_hashes(
                    &p2_hashes,
                    self.request.include_spent,
                    self.window.effective_start_height,
                    self.window.effective_end_height,
                ),
                self.scanner.by_hints(
                    &p2_hashes,
                    self.request.include_spent,
                    self.window.effective_start_height,
                    self.window.effective_end_height,
                ),
            );
            let by_puzzle = by_puzzle?;
            let by_hint = by_hint?;

            let batch_has_any = !by_puzzle.is_empty() || !by_hint.is_empty();
            if batch_end > 0 && !batch_has_any {
                empty_batch_count = empty_batch_count.saturating_add(1);
            } else {
                empty_batch_count = 0;
            }
            if empty_batch_count >= empty_batch_stop_count {
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
                self.checkpoint.nonce_to_p2.insert(*nonce, normalized);
            }
        }
        Ok(batch_nonce_p2)
    }
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

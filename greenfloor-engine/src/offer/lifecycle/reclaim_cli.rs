//! CLI: reclaim orphaned presplit maker coins by coin id + fixed delegated hash.

use serde::Serialize;
use serde_json::{json, Value};

use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::offer::reclaim::reclaim_presplit_maker_coin;

/// One `--coin-id` / `--fixed-delegated-puzzle-hash` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresplitReclaimPair {
    pub coin_id: String,
    pub fixed_delegated_puzzle_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OffersReclaimPresplitCliItem {
    pub coin_id: String,
    pub fixed_delegated_puzzle_hash: String,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct OffersReclaimPresplitCliResult {
    pub dry_run: bool,
    pub selected_count: u64,
    pub submitted_count: u64,
    pub failed_count: u64,
    pub items: Vec<OffersReclaimPresplitCliItem>,
}

/// Zip repeatable CLI flags into 1:1 reclaim pairs (normalized hex).
///
/// # Errors
///
/// Returns an error when either list is empty or the lengths differ.
pub fn parse_presplit_reclaim_pairs(
    coin_ids: &[String],
    fixed_hashes: &[String],
) -> SignerResult<Vec<PresplitReclaimPair>> {
    if coin_ids.is_empty() || fixed_hashes.is_empty() {
        return Err(SignerError::Other(
            "provide at least one --coin-id and --fixed-delegated-puzzle-hash pair".to_string(),
        ));
    }
    if coin_ids.len() != fixed_hashes.len() {
        return Err(SignerError::Other(format!(
            "--coin-id count ({}) must match --fixed-delegated-puzzle-hash count ({})",
            coin_ids.len(),
            fixed_hashes.len()
        )));
    }
    let mut pairs = Vec::with_capacity(coin_ids.len());
    for (coin_id, fixed_hash) in coin_ids.iter().zip(fixed_hashes.iter()) {
        let coin_id = normalize_hex_id(coin_id);
        let fixed_delegated_puzzle_hash = normalize_hex_id(fixed_hash);
        if coin_id.is_empty() || fixed_delegated_puzzle_hash.is_empty() {
            return Err(SignerError::Other(
                "each --coin-id and --fixed-delegated-puzzle-hash must be non-empty hex"
                    .to_string(),
            ));
        }
        if coin_id.len() != 64 || fixed_delegated_puzzle_hash.len() != 64 {
            return Err(SignerError::Other(
                "each --coin-id and --fixed-delegated-puzzle-hash must be 64 hex characters"
                    .to_string(),
            ));
        }
        pairs.push(PresplitReclaimPair {
            coin_id,
            fixed_delegated_puzzle_hash,
        });
    }
    Ok(pairs)
}

fn item_success(
    pair: &PresplitReclaimPair,
    operation_id: &str,
    dry_run: bool,
) -> OffersReclaimPresplitCliItem {
    OffersReclaimPresplitCliItem {
        coin_id: pair.coin_id.clone(),
        fixed_delegated_puzzle_hash: pair.fixed_delegated_puzzle_hash.clone(),
        result: json!({
            "success": true,
            "dry_run": dry_run,
            "operation_id": operation_id,
            "error": "",
        }),
    }
}

fn item_failure(pair: &PresplitReclaimPair, error: &str) -> OffersReclaimPresplitCliItem {
    OffersReclaimPresplitCliItem {
        coin_id: pair.coin_id.clone(),
        fixed_delegated_puzzle_hash: pair.fixed_delegated_puzzle_hash.clone(),
        result: json!({
            "success": false,
            "operation_id": "",
            "error": error,
        }),
    }
}

/// Reclaim orphaned presplit maker coins (ephemeral: no `offer_state` row).
///
/// Builds a cancel/reclaim spend per pair via stored metadata. With `dry_run`, builds and
/// signs but does not `push_tx`.
///
/// # Errors
///
/// Returns an error when pair parsing fails. Per-coin reclaim failures are soft-failed into
/// the result payload (exit code decided by the manager wrapper).
pub async fn offers_reclaim_presplit_cli(
    signer_config: SignerConfig,
    operator_network: &str,
    coin_ids: &[String],
    fixed_hashes: &[String],
    dry_run: bool,
) -> SignerResult<OffersReclaimPresplitCliResult> {
    let pairs = parse_presplit_reclaim_pairs(coin_ids, fixed_hashes)?;

    let mut items = Vec::with_capacity(pairs.len());
    let mut submitted_count = 0u64;
    let mut failed_count = 0u64;

    for pair in &pairs {
        match reclaim_presplit_maker_coin(
            signer_config.clone(),
            operator_network,
            &pair.coin_id,
            &pair.fixed_delegated_puzzle_hash,
            dry_run,
        )
        .await
        {
            Ok(operation_id) => {
                submitted_count = submitted_count.saturating_add(1);
                items.push(item_success(pair, &operation_id, dry_run));
            }
            Err(err) => {
                failed_count = failed_count.saturating_add(1);
                items.push(item_failure(pair, &err.to_string()));
            }
        }
    }

    Ok(OffersReclaimPresplitCliResult {
        dry_run,
        selected_count: u64::try_from(pairs.len()).unwrap_or(u64::MAX),
        submitted_count,
        failed_count,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pairs_accepts_0x_prefix() {
        let coin = format!("0x{}", "ab".repeat(32));
        let fixed = format!("0x{}", "cd".repeat(32));
        let pairs = parse_presplit_reclaim_pairs(&[coin], &[fixed]).expect("pairs");
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].coin_id, "ab".repeat(32));
        assert_eq!(pairs[0].fixed_delegated_puzzle_hash, "cd".repeat(32));
    }

    #[test]
    fn parse_pairs_rejects_length_mismatch() {
        let err =
            parse_presplit_reclaim_pairs(&["aa".repeat(32)], &["bb".repeat(32), "cc".repeat(32)])
                .expect_err("mismatch");
        assert!(err.to_string().contains("must match"));
    }

    #[test]
    fn parse_pairs_rejects_empty() {
        let err = parse_presplit_reclaim_pairs(&[], &[]).expect_err("empty");
        assert!(err.to_string().contains("at least one"));
    }
}

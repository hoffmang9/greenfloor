//! CLI: reclaim orphaned presplit maker coins by coin id + fixed delegated hash.

use serde::Serialize;

use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex;
use crate::offer::reclaim::reclaim_presplit_maker_coin;

/// One `--coin-id` / `--fixed-delegated-puzzle-hash` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresplitReclaimPair {
    pub coin_id: String,
    pub fixed_delegated_puzzle_hash: String,
}

/// Typed per-coin reclaim outcome (serialized under `result`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OffersReclaimPresplitOutcome {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    pub operation_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OffersReclaimPresplitCliItem {
    pub coin_id: String,
    pub fixed_delegated_puzzle_hash: String,
    pub result: OffersReclaimPresplitOutcome,
}

impl OffersReclaimPresplitCliItem {
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.result.success
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.result.operation_id
    }

    #[must_use]
    pub fn error_message(&self) -> &str {
        &self.result.error
    }

    #[must_use]
    pub fn success(pair: &PresplitReclaimPair, operation_id: &str, dry_run: bool) -> Self {
        Self {
            coin_id: pair.coin_id.clone(),
            fixed_delegated_puzzle_hash: pair.fixed_delegated_puzzle_hash.clone(),
            result: OffersReclaimPresplitOutcome {
                success: true,
                dry_run: Some(dry_run),
                operation_id: operation_id.to_string(),
                error: String::new(),
            },
        }
    }

    #[must_use]
    pub fn failure(pair: &PresplitReclaimPair, error: &str) -> Self {
        Self {
            coin_id: pair.coin_id.clone(),
            fixed_delegated_puzzle_hash: pair.fixed_delegated_puzzle_hash.clone(),
            result: OffersReclaimPresplitOutcome {
                success: false,
                dry_run: None,
                operation_id: String::new(),
                error: error.to_string(),
            },
        }
    }
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
        let coin_id = normalize_hex(coin_id);
        let fixed_delegated_puzzle_hash = normalize_hex(fixed_hash);
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

/// Reclaim orphaned presplit maker coins from already-normalized pairs.
///
/// # Errors
///
/// Per-coin reclaim failures are soft-failed into the result payload (exit code decided by
/// the manager wrapper). This function itself only fails on unexpected orchestration errors.
pub async fn offers_reclaim_presplit_pairs(
    signer_config: SignerConfig,
    operator_network: &str,
    pairs: &[PresplitReclaimPair],
    dry_run: bool,
) -> SignerResult<OffersReclaimPresplitCliResult> {
    let mut items = Vec::with_capacity(pairs.len());
    let mut submitted_count = 0u64;
    let mut failed_count = 0u64;

    for pair in pairs {
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
                items.push(OffersReclaimPresplitCliItem::success(
                    pair,
                    &operation_id,
                    dry_run,
                ));
            }
            Err(err) => {
                failed_count = failed_count.saturating_add(1);
                items.push(OffersReclaimPresplitCliItem::failure(
                    pair,
                    &err.to_string(),
                ));
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
    offers_reclaim_presplit_pairs(signer_config, operator_network, &pairs, dry_run).await
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

    #[test]
    fn parse_pairs_rejects_empty_or_short_hex() {
        let err = parse_presplit_reclaim_pairs(&[String::new()], &["aa".repeat(32)])
            .expect_err("empty coin");
        assert!(err.to_string().contains("non-empty"));
        let err = parse_presplit_reclaim_pairs(&["aa".repeat(16)], &["bb".repeat(32)])
            .expect_err("short");
        assert!(err.to_string().contains("64 hex"));
    }

    #[test]
    fn item_success_and_failure_payload_shapes() {
        let pair = PresplitReclaimPair {
            coin_id: "aa".repeat(32),
            fixed_delegated_puzzle_hash: "bb".repeat(32),
        };
        let ok = OffersReclaimPresplitCliItem::success(&pair, "op-1", true);
        assert!(ok.succeeded());
        assert_eq!(ok.result.dry_run, Some(true));
        assert_eq!(ok.operation_id(), "op-1");
        let fail = OffersReclaimPresplitCliItem::failure(&pair, "boom");
        assert!(!fail.succeeded());
        assert_eq!(fail.error_message(), "boom");
        assert_eq!(fail.result.dry_run, None);
    }
}

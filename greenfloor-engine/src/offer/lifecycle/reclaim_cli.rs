//! CLI: reclaim orphaned presplit maker coins by coin id + fixed delegated hash.

use serde::Serialize;

use crate::coinset::{client_for_signer_on_network, wait_until_coins_spent, CoinSpentVerifyConfig};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, normalize_hex};
use crate::offer::reclaim::reclaim_presplit_maker_coin;

/// One `--coin-id` / `--fixed-delegated-puzzle-hash` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresplitReclaimPair {
    pub coin_id: String,
    pub fixed_delegated_puzzle_hash: String,
}

/// Whether to confirm each submitted reclaim before starting the next (vault singleton safety).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReclaimConfirmWait {
    /// Do not wait between submits (dry-run, or explicit `--no-wait`).
    #[default]
    None,
    /// Wait until each submitted maker is spent; stop the batch on wait failure.
    AfterEachSubmit,
}

/// Typed per-coin reclaim outcome (serialized under `result`).
///
/// Invariants: `error` is set only when `success` is false (reclaim/submit failure).
/// `wait_error` is independent and may be set after a successful submit when confirm-wait fails.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OffersReclaimPresplitOutcome {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    pub operation_id: String,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_error: Option<String>,
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
                wait_error: None,
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
                wait_error: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OffersReclaimPresplitCliResult {
    pub dry_run: bool,
    /// Pairs attempted (may be less than requested when confirm-wait stops the batch).
    pub selected_count: u64,
    /// Pairs not attempted because the batch stopped early.
    pub remaining_count: u64,
    pub submitted_count: u64,
    pub failed_count: u64,
    pub stopped_early: bool,
    pub items: Vec<OffersReclaimPresplitCliItem>,
}

impl OffersReclaimPresplitCliResult {
    /// True when any reclaim submit failed or a post-submit confirm-wait failed.
    #[must_use]
    pub fn cli_failed(&self) -> bool {
        self.failed_count > 0
            || self
                .items
                .iter()
                .any(|item| item.result.wait_error.is_some())
    }
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

#[must_use]
pub(crate) fn confirm_wait_for_run(dry_run: bool, wait: bool) -> ReclaimConfirmWait {
    if dry_run || !wait {
        ReclaimConfirmWait::None
    } else {
        ReclaimConfirmWait::AfterEachSubmit
    }
}

/// Reclaim orphaned presplit maker coins from already-normalized pairs.
///
/// When `confirm_wait` is [`ReclaimConfirmWait::AfterEachSubmit`], each successful broadcast
/// waits for the maker coin to be spent before the next pair (vault singleton serialization).
/// A wait failure sets `wait_error` and stops the batch; `remaining_count` lists unattempted pairs.
///
/// # Errors
///
/// Per-coin reclaim failures are soft-failed into the result payload (exit code decided by
/// the manager wrapper). This function itself only fails on unexpected orchestration errors
/// (for example Coinset client construction when waiting).
pub async fn offers_reclaim_presplit_pairs(
    signer_config: SignerConfig,
    operator_network: &str,
    pairs: &[PresplitReclaimPair],
    dry_run: bool,
    confirm_wait: ReclaimConfirmWait,
) -> SignerResult<OffersReclaimPresplitCliResult> {
    let wait_client = if matches!(confirm_wait, ReclaimConfirmWait::AfterEachSubmit) {
        Some(client_for_signer_on_network(
            &signer_config,
            operator_network,
        )?)
    } else {
        None
    };

    let mut items = Vec::with_capacity(pairs.len());
    let mut submitted_count = 0u64;
    let mut failed_count = 0u64;
    let mut stopped_early = false;

    for (index, pair) in pairs.iter().enumerate() {
        let mut item = match reclaim_presplit_maker_coin(
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
                OffersReclaimPresplitCliItem::success(pair, &operation_id, dry_run)
            }
            Err(err) => {
                failed_count = failed_count.saturating_add(1);
                OffersReclaimPresplitCliItem::failure(pair, &err.to_string())
            }
        };

        if let Some(client) = wait_client.as_ref() {
            if item.succeeded() {
                let wait_err = match hex_to_bytes32(&pair.coin_id) {
                    Ok(coin) => {
                        wait_until_coins_spent(client, &[coin], CoinSpentVerifyConfig::default())
                            .await
                            .err()
                    }
                    Err(err) => Some(err),
                };
                if let Some(err) = wait_err {
                    item.result.wait_error = Some(err.to_string());
                    items.push(item);
                    stopped_early = index + 1 < pairs.len();
                    break;
                }
            }
        }

        items.push(item);
    }

    let selected_count = u64::try_from(items.len()).unwrap_or(u64::MAX);
    let remaining_count = u64::try_from(pairs.len().saturating_sub(items.len())).unwrap_or(0);

    Ok(OffersReclaimPresplitCliResult {
        dry_run,
        selected_count,
        remaining_count,
        submitted_count,
        failed_count,
        stopped_early,
        items,
    })
}

/// Reclaim orphaned presplit maker coins (ephemeral: no `offer_state` row).
///
/// Builds a cancel/reclaim spend per pair via stored metadata. With `dry_run`, builds and
/// signs but does not `push_tx`. When `wait` is true and not dry-run, confirms each submit
/// before the next pair.
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
    wait: bool,
) -> SignerResult<OffersReclaimPresplitCliResult> {
    let pairs = parse_presplit_reclaim_pairs(coin_ids, fixed_hashes)?;
    offers_reclaim_presplit_pairs(
        signer_config,
        operator_network,
        &pairs,
        dry_run,
        confirm_wait_for_run(dry_run, wait),
    )
    .await
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
    fn confirm_wait_disabled_for_dry_run_or_no_wait() {
        assert_eq!(confirm_wait_for_run(true, true), ReclaimConfirmWait::None);
        assert_eq!(confirm_wait_for_run(false, false), ReclaimConfirmWait::None);
        assert_eq!(
            confirm_wait_for_run(false, true),
            ReclaimConfirmWait::AfterEachSubmit
        );
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
        assert_eq!(ok.result.wait_error, None);
        let fail = OffersReclaimPresplitCliItem::failure(&pair, "boom");
        assert!(!fail.succeeded());
        assert_eq!(fail.error_message(), "boom");
        assert_eq!(fail.result.dry_run, None);
        assert_eq!(fail.result.wait_error, None);

        let mut waited = OffersReclaimPresplitCliItem::success(&pair, "op-1", false);
        waited.result.wait_error = Some("timeout".to_string());
        let with_wait = OffersReclaimPresplitCliResult {
            dry_run: false,
            selected_count: 1,
            remaining_count: 1,
            submitted_count: 1,
            failed_count: 0,
            stopped_early: true,
            items: vec![waited],
        };
        assert!(with_wait.cli_failed());
        assert_eq!(with_wait.remaining_count, 1);
    }
}

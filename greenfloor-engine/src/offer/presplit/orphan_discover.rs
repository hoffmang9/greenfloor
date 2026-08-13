//! Discover orphaned vault-hinted presplit maker coins and recover fixed hashes.

use std::collections::HashSet;

use chia_protocol::{Bytes32, CoinSpend};
use chia_sdk_coinset::CoinsetClient;
use clvm_traits::FromClvm;
use clvm_utils::{tree_hash, TreeHash};
use clvmr::serde::node_from_bytes;
use clvmr::{Allocator, NodePtr};

use crate::coinset::{cat_child_p2_create_coin_memos, fetch_parent_coin_spend};
use crate::error::{SignerError, SignerResult, TransportError};
use crate::hex::{hex_to_bytes32, normalize_hex_id, tree_hash_to_hex};
use crate::offer::presplit::resolve_member_fixed_conditions_hash_for_binding;
use crate::vault_coinset_scan::types::CoinRow;

/// Minimal scan view for orphan discovery (decoupled from vault-scan `CoinRow` extras).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanScanRow {
    pub coin_id: String,
    pub parent_coin_info: String,
    pub amount: u64,
    pub confirmed_block_index: u64,
    pub spent_block_index: u64,
    pub discovered_by_hint: bool,
    pub discovered_by_puzzle_hash: bool,
    pub cat_asset_id: Option<String>,
}

impl From<&CoinRow> for OrphanScanRow {
    fn from(row: &CoinRow) -> Self {
        Self {
            coin_id: row.coin_id.clone(),
            parent_coin_info: row.parent_coin_info.clone(),
            amount: row.amount,
            confirmed_block_index: row.confirmed_block_index,
            spent_block_index: row.spent_block_index,
            discovered_by_hint: row.discovered_by_hint,
            discovered_by_puzzle_hash: row.discovered_by_puzzle_hash,
            cat_asset_id: row.cat_asset_id.clone(),
        }
    }
}

/// One unspent vault-hinted CAT coin that may be a stranded presplit maker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanPresplitCandidate {
    pub coin_id: String,
    pub amount: u64,
    pub confirmed_block_index: u64,
    /// Binding-valid fixed hash when recovered from CW-style parent memos.
    pub fixed_delegated_puzzle_hash: Option<String>,
    pub recovery_detail: String,
}

impl OrphanPresplitCandidate {
    #[must_use]
    pub fn recovery_label(&self) -> &'static str {
        if self.fixed_delegated_puzzle_hash.is_some() {
            "parent_memo"
        } else {
            "unavailable"
        }
    }
}

/// Select unspent CAT scan rows that look like hinted maker coins (not vault-nonce receive).
#[must_use]
pub fn hinted_unspent_cat_candidates<'a>(
    rows: &'a [OrphanScanRow],
    asset_id: &str,
    tracked_coin_ids: &HashSet<String>,
    start_height: Option<u64>,
) -> Vec<&'a OrphanScanRow> {
    let asset = normalize_hex_id(asset_id);
    rows.iter()
        .filter(|row| {
            row.spent_block_index == 0
                && row.discovered_by_hint
                && !row.discovered_by_puzzle_hash
                && row
                    .cat_asset_id
                    .as_deref()
                    .is_some_and(|id| normalize_hex_id(id) == asset)
                && start_height.is_none_or(|min| row.confirmed_block_index >= min)
                && !tracked_coin_ids.contains(&normalize_hex_id(&row.coin_id))
        })
        .collect()
}

fn second_memo_item(allocator: &Allocator, memos_ptr: NodePtr) -> Option<NodePtr> {
    let Ok((first, rest)) = <(NodePtr, NodePtr)>::from_clvm(allocator, memos_ptr) else {
        return None;
    };
    let _ = first;
    if let Ok((second, _nil)) = <(NodePtr, NodePtr)>::from_clvm(allocator, rest) {
        return Some(second);
    }
    // Improper list: rest is the second item directly.
    Some(rest)
}

fn candidate_fixed_hashes_in_allocator(
    allocator: &mut Allocator,
    conditions_ptr: NodePtr,
) -> SignerResult<[TreeHash; 2]> {
    let direct = tree_hash(allocator, conditions_ptr);
    let q = allocator
        .new_atom(&[1])
        .map_err(|err| SignerError::driver(err.to_string()))?;
    let quoted = allocator
        .new_pair(q, conditions_ptr)
        .map_err(|err| SignerError::driver(err.to_string()))?;
    Ok([direct, tree_hash(allocator, quoted)])
}

/// Recover a binding-valid fixed hash from a CAT parent spend's inner p2 `CREATE_COIN` memos.
///
/// Cloud Wallet emits `createCoin(p2PuzzleHash, amount, [changePuzzleHash, fixedConditions])`.
/// Those conditions live on the **inner** p2 spend, not on the outer CAT puzzle output.
pub(crate) fn recover_from_parent_spend(
    launcher_id: Bytes32,
    coin_id: Bytes32,
    amount: u64,
    parent_spend: &CoinSpend,
) -> SignerResult<(Option<TreeHash>, String)> {
    let Some((cat, memos_bytes)) = cat_child_p2_create_coin_memos(parent_spend, coin_id)? else {
        return Ok((
            None,
            "CREATE_COIN for child not found in parent CAT inner spend".to_string(),
        ));
    };
    if cat.coin.amount != amount {
        return Ok((None, "CREATE_COIN amount mismatch".to_string()));
    }
    let Some(memos_bytes) = memos_bytes else {
        return Ok((
            None,
            "CREATE_COIN has no memos (GF-native split)".to_string(),
        ));
    };
    let mut allocator = Allocator::new();
    let memos_ptr = node_from_bytes(&mut allocator, &memos_bytes)
        .map_err(|err| SignerError::driver(err.to_string()))?;
    let Some(conditions_ptr) = second_memo_item(&allocator, memos_ptr) else {
        return Ok((
            None,
            "CREATE_COIN memos are not a two-item CW-style list".to_string(),
        ));
    };
    for candidate in candidate_fixed_hashes_in_allocator(&mut allocator, conditions_ptr)? {
        if resolve_member_fixed_conditions_hash_for_binding(
            launcher_id,
            cat.info.p2_puzzle_hash,
            candidate,
        )
        .is_ok()
        {
            return Ok((Some(candidate), "parent_memo".to_string()));
        }
    }
    Ok((
        None,
        "memo fixed-hash candidates did not match binding p2".to_string(),
    ))
}

async fn recover_fixed_hash_for_row(
    client: &CoinsetClient,
    launcher_id: Bytes32,
    coin_id: Bytes32,
    amount: u64,
    parent_coin_info: Bytes32,
) -> SignerResult<(Option<TreeHash>, String)> {
    let Some(parent_spend) = fetch_parent_coin_spend(client, parent_coin_info).await? else {
        return Ok((
            None,
            "orphan parent coin record or spend not found".to_string(),
        ));
    };
    recover_from_parent_spend(launcher_id, coin_id, amount, &parent_spend)
}

fn soft_fail_detail(err: SignerError) -> SignerResult<String> {
    match err {
        // Transport / API failures should fail the whole discover, not hide as per-row noise.
        SignerError::Transport(TransportError::Coinset(_)) => Err(err),
        other => Ok(other.to_string()),
    }
}

/// Build orphan candidates with recovered fixed hashes where possible.
///
/// Per-coin logical recovery misses (invalid ids, missing memos, parse failures) are soft:
/// the candidate is still returned with `fixed_delegated_puzzle_hash = None`.
/// Coinset transport failures abort discovery.
///
/// # Errors
///
/// Returns [`crate::error::TransportError::Coinset`] when Coinset access fails for a candidate.
pub async fn discover_orphan_presplit_candidates(
    client: &CoinsetClient,
    launcher_id: Bytes32,
    rows: &[OrphanScanRow],
    asset_id: &str,
    tracked_coin_ids: &HashSet<String>,
    start_height: Option<u64>,
) -> SignerResult<Vec<OrphanPresplitCandidate>> {
    let mut out = Vec::new();
    for row in hinted_unspent_cat_candidates(rows, asset_id, tracked_coin_ids, start_height) {
        let coin_id = normalize_hex_id(&row.coin_id);
        let parent = normalize_hex_id(&row.parent_coin_info);
        let (hash, detail) = match (hex_to_bytes32(&coin_id), hex_to_bytes32(&parent)) {
            (Ok(coin_bytes), Ok(parent_bytes)) => {
                match recover_fixed_hash_for_row(
                    client,
                    launcher_id,
                    coin_bytes,
                    row.amount,
                    parent_bytes,
                )
                .await
                {
                    Ok(recovered) => recovered,
                    Err(err) => (None, soft_fail_detail(err)?),
                }
            }
            (Err(err), _) | (_, Err(err)) => (None, err.to_string()),
        };
        out.push(OrphanPresplitCandidate {
            coin_id,
            amount: row.amount,
            confirmed_block_index: row.confirmed_block_index,
            fixed_delegated_puzzle_hash: hash.map(tree_hash_to_hex),
            recovery_detail: detail,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        coin_id: &str,
        hint: bool,
        by_ph: bool,
        spent: u64,
        asset: &str,
        height: u64,
    ) -> OrphanScanRow {
        OrphanScanRow {
            coin_id: coin_id.to_string(),
            parent_coin_info: "cc".repeat(32),
            amount: 1000,
            confirmed_block_index: height,
            spent_block_index: spent,
            discovered_by_puzzle_hash: by_ph,
            discovered_by_hint: hint,
            cat_asset_id: Some(asset.to_string()),
        }
    }

    #[test]
    fn hinted_candidates_exclude_receive_tracked_spent_and_low_height() {
        let asset = "aa".repeat(32);
        let tracked = HashSet::from(["dd".repeat(32)]);
        let rows = vec![
            row(&"11".repeat(32), true, false, 0, &asset, 9_000_000),
            row(&"22".repeat(32), true, true, 0, &asset, 9_000_000),
            row(&"33".repeat(32), false, false, 0, &asset, 9_000_000),
            row(&"44".repeat(32), true, false, 10, &asset, 9_000_000),
            row(&"dd".repeat(32), true, false, 0, &asset, 9_000_000),
            row(&"55".repeat(32), true, false, 0, &asset, 100),
            row(
                &"66".repeat(32),
                true,
                false,
                0,
                &"ee".repeat(32),
                9_000_000,
            ),
        ];
        let got = hinted_unspent_cat_candidates(&rows, &asset, &tracked, Some(8_330_000));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].coin_id, "11".repeat(32));
    }

    #[test]
    fn quote_and_direct_candidate_hashes_differ() {
        let mut allocator = Allocator::new();
        let atom = allocator.new_atom(&[0xab, 0xcd]).expect("atom");
        let conditions = allocator.new_pair(atom, NodePtr::NIL).expect("pair");
        let hashes = candidate_fixed_hashes_in_allocator(&mut allocator, conditions).expect("hash");
        assert_ne!(hashes[0], hashes[1]);
    }

    #[test]
    fn recovery_label_from_hash_presence() {
        let with_hash = OrphanPresplitCandidate {
            coin_id: "11".repeat(32),
            amount: 1000,
            confirmed_block_index: 1,
            fixed_delegated_puzzle_hash: Some("aa".repeat(32)),
            recovery_detail: "parent_memo".to_string(),
        };
        let without = OrphanPresplitCandidate {
            fixed_delegated_puzzle_hash: None,
            recovery_detail: "CREATE_COIN has no memos (GF-native split)".to_string(),
            ..with_hash.clone()
        };
        assert_eq!(with_hash.recovery_label(), "parent_memo");
        assert_eq!(without.recovery_label(), "unavailable");
    }

    #[test]
    fn soft_fail_detail_propagates_coinset_transport() {
        let err = soft_fail_detail(SignerError::coinset("down".to_string())).expect_err("coinset");
        assert!(matches!(
            err,
            SignerError::Transport(TransportError::Coinset(_))
        ));
        let detail = soft_fail_detail(SignerError::driver("bad puzzle".to_string())).expect("soft");
        assert!(detail.contains("bad puzzle"));
    }

    #[test]
    fn orphan_scan_row_from_coin_row() {
        let coin = CoinRow {
            coin_id: "11".repeat(32),
            puzzle_hash: "22".repeat(32),
            parent_coin_info: "33".repeat(32),
            amount: 5000,
            confirmed_block_index: 9,
            spent_block_index: 0,
            discovered_nonces: vec![1],
            discovered_by_puzzle_hash: false,
            discovered_by_hint: true,
            kind: crate::vault_coinset_scan::types::CoinKind::Cat,
            cat_asset_id: Some("aa".repeat(32)),
            cat_symbols: vec!["BYC".to_string()],
        };
        let row = OrphanScanRow::from(&coin);
        assert_eq!(row.coin_id, coin.coin_id);
        assert_eq!(row.parent_coin_info, coin.parent_coin_info);
        assert_eq!(row.amount, 5000);
        assert!(row.discovered_by_hint);
        assert_eq!(row.cat_asset_id.as_deref(), Some("aa".repeat(32).as_str()));
    }

    #[tokio::test]
    async fn discover_soft_fails_invalid_coin_hex_and_continues() {
        let asset = "aa".repeat(32);
        let tracked = HashSet::new();
        let bad = OrphanScanRow {
            coin_id: "not-hex".to_string(),
            parent_coin_info: "cc".repeat(32),
            amount: 1000,
            confirmed_block_index: 9_000_000,
            spent_block_index: 0,
            discovered_by_puzzle_hash: false,
            discovered_by_hint: true,
            cat_asset_id: Some(asset.clone()),
        };
        // No Coinset calls: invalid hex soft-fails before network.
        let client = CoinsetClient::new("https://api.coinset.org".to_string());
        let got = discover_orphan_presplit_candidates(
            &client,
            Bytes32::new([0u8; 32]),
            &[bad],
            &asset,
            &tracked,
            None,
        )
        .await
        .expect("soft fail");
        assert_eq!(got.len(), 1);
        assert!(got[0].fixed_delegated_puzzle_hash.is_none());
        assert_eq!(got[0].recovery_label(), "unavailable");
        assert!(!got[0].recovery_detail.is_empty());
    }
}

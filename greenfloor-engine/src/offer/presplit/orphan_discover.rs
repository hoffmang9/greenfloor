//! Discover orphaned vault-hinted presplit maker coins and recover fixed hashes.

use std::collections::HashSet;

use chia_protocol::{Bytes32, Coin, CoinSpend};
use chia_puzzle_types::Memos;
use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient, GetPuzzleAndSolutionResponse};
use chia_sdk_types::{run_puzzle, Condition, Conditions};
use clvm_traits::FromClvm;
use clvm_utils::{tree_hash, TreeHash};
use clvmr::serde::node_from_bytes;
use clvmr::{Allocator, NodePtr};

use crate::coinset::{cat_from_parent_spend, with_coinset_client_retries};
use crate::error::{SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, normalize_hex_id, tree_hash_to_hex};
use crate::offer::presplit::resolve_member_fixed_conditions_hash_for_binding;
use crate::vault_coinset_scan::types::{CoinKind, CoinRow};

/// How a fixed-conditions hash was recovered for an orphan candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanFixedHashRecovery {
    ParentMemo,
    Unavailable,
}

/// One unspent vault-hinted CAT coin that may be a stranded presplit maker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanPresplitCandidate {
    pub coin_id: String,
    pub amount: u64,
    pub confirmed_block_index: u64,
    pub fixed_delegated_puzzle_hash: Option<String>,
    pub recovery_detail: String,
}

impl OrphanPresplitCandidate {
    #[must_use]
    pub fn recovery(&self) -> OrphanFixedHashRecovery {
        if self.fixed_delegated_puzzle_hash.is_some() {
            OrphanFixedHashRecovery::ParentMemo
        } else {
            OrphanFixedHashRecovery::Unavailable
        }
    }
}

/// Select unspent CAT scan rows that look like hinted maker coins (not vault-nonce receive).
#[must_use]
pub fn hinted_unspent_cat_candidates<'a>(
    rows: &'a [CoinRow],
    asset_id: &str,
    tracked_coin_ids: &HashSet<String>,
    start_height: Option<u64>,
) -> Vec<&'a CoinRow> {
    let asset = normalize_hex_id(asset_id);
    rows.iter()
        .filter(|row| {
            row.kind == CoinKind::Cat
                && row.spent_block_index == 0
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
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let quoted = allocator
        .new_pair(q, conditions_ptr)
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    Ok([direct, tree_hash(allocator, quoted)])
}

/// Recover a binding-valid fixed hash from a parent spend in one puzzle pass.
fn recover_from_parent_spend(
    launcher_id: Bytes32,
    coin_id: Bytes32,
    amount: u64,
    parent_spend: &CoinSpend,
) -> SignerResult<(Option<TreeHash>, String)> {
    let mut allocator = Allocator::new();
    let puzzle = node_from_bytes(&mut allocator, parent_spend.puzzle_reveal.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let solution = node_from_bytes(&mut allocator, parent_spend.solution.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let output = run_puzzle(&mut allocator, puzzle, solution)
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let conditions = Conditions::<NodePtr>::from_clvm(&allocator, output)
        .map_err(|err| SignerError::Driver(err.to_string()))?;

    let mut child_coin = None;
    let mut conditions_ptr = None;
    for condition in conditions.iter() {
        let Condition::CreateCoin(create) = condition else {
            continue;
        };
        let created = Coin::new(
            parent_spend.coin.coin_id(),
            create.puzzle_hash,
            create.amount,
        );
        if created.coin_id() != coin_id {
            continue;
        }
        conditions_ptr = match create.memos {
            Memos::Some(ptr) => second_memo_item(&allocator, ptr),
            Memos::None => None,
        };
        child_coin = Some(created);
        break;
    }
    let Some(child_coin) = child_coin else {
        return Ok((
            None,
            "CREATE_COIN for child not found in parent spend".to_string(),
        ));
    };
    if child_coin.amount != amount {
        return Ok((None, "CREATE_COIN amount mismatch".to_string()));
    }
    let Some(cat) = cat_from_parent_spend(child_coin, parent_spend)? else {
        return Ok((
            None,
            "parent spend does not yield matching CAT child".to_string(),
        ));
    };
    if cat.coin.coin_id() != coin_id {
        return Ok((None, "CAT child coin id mismatch".to_string()));
    }
    let Some(conditions_ptr) = conditions_ptr else {
        return Ok((
            None,
            "CREATE_COIN has no memos (GF-native split) or memos are not a two-item CW-style list"
                .to_string(),
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
    let parent_record = with_coinset_client_retries(|| async {
        client.get_coin_record_by_name(parent_coin_info).await
    })
    .await?
    .coin_record;
    let Some(parent_record) = parent_record else {
        return Ok((None, "orphan parent coin record not found".to_string()));
    };
    if parent_record.spent_block_index == 0 {
        return Ok((None, "parent coin is unspent".to_string()));
    }
    let parent_coin_id = parent_record.coin.coin_id();
    let solution_response: GetPuzzleAndSolutionResponse = with_coinset_client_retries(|| async {
        client
            .get_puzzle_and_solution(parent_coin_id, Some(parent_record.spent_block_index))
            .await
    })
    .await?;
    let Some(parent_spend) = solution_response.coin_solution else {
        return Ok((None, "parent puzzle and solution missing".to_string()));
    };
    recover_from_parent_spend(launcher_id, coin_id, amount, &parent_spend)
}

/// Build orphan candidates with recovered fixed hashes where possible.
///
/// Coinset transport failures fail the whole discovery. Per-coin recovery misses are soft:
/// the candidate is still returned with `fixed_delegated_puzzle_hash = None`.
///
/// # Errors
///
/// Returns an error when Coinset access fails for a candidate in a non-recoverable way.
pub async fn discover_orphan_presplit_candidates(
    client: &CoinsetClient,
    launcher_id: Bytes32,
    rows: &[CoinRow],
    asset_id: &str,
    tracked_coin_ids: &HashSet<String>,
    start_height: Option<u64>,
) -> SignerResult<Vec<OrphanPresplitCandidate>> {
    let mut out = Vec::new();
    for row in hinted_unspent_cat_candidates(rows, asset_id, tracked_coin_ids, start_height) {
        let coin_id = normalize_hex_id(&row.coin_id);
        let parent = normalize_hex_id(&row.parent_coin_info);
        let coin_bytes = hex_to_bytes32(&coin_id)?;
        let parent_bytes = hex_to_bytes32(&parent)?;
        let (hash, detail) =
            recover_fixed_hash_for_row(client, launcher_id, coin_bytes, row.amount, parent_bytes)
                .await?;
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
    ) -> CoinRow {
        CoinRow {
            coin_id: coin_id.to_string(),
            puzzle_hash: "bb".repeat(32),
            parent_coin_info: "cc".repeat(32),
            amount: 1000,
            confirmed_block_index: height,
            spent_block_index: spent,
            discovered_nonces: vec![0],
            discovered_by_puzzle_hash: by_ph,
            discovered_by_hint: hint,
            kind: CoinKind::Cat,
            cat_asset_id: Some(asset.to_string()),
            cat_symbols: vec![],
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
    fn recovery_derived_from_hash_presence() {
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
        assert_eq!(with_hash.recovery(), OrphanFixedHashRecovery::ParentMemo);
        assert_eq!(without.recovery(), OrphanFixedHashRecovery::Unavailable);
    }
}

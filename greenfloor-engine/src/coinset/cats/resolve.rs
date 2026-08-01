use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use chia_protocol::{Bytes32, Coin, CoinSpend};
use chia_puzzle_types::Memos;
use chia_sdk_coinset::{
    ChiaRpcClient, CoinRecord, CoinsetClient, GetCoinRecordResponse, GetPuzzleAndSolutionResponse,
};
use chia_sdk_driver::{Cat, CatInfo, CatLayer, Layer, Puzzle, RevocationLayer, Spend};
use chia_sdk_types::{run_puzzle, Condition};
use clvm_traits::FromClvm;
use clvmr::serde::{node_from_bytes, node_to_bytes};
use clvmr::{Allocator, NodePtr};

use crate::coinset::retry::with_coinset_client_retries;
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;

fn unparseable_cat_lineage(detail: impl Into<String>) -> SignerError {
    SignerError::UnparseableCatLineage(detail.into())
}

/// Fetch the parent coin's spend (record must be spent and have a puzzle/solution).
///
/// # Errors
///
/// Returns [`SignerError::Coinset`] on transport/API failure.
pub async fn fetch_parent_coin_spend(
    client: &CoinsetClient,
    parent_coin_info: Bytes32,
) -> SignerResult<Option<CoinSpend>> {
    let parent_response: GetCoinRecordResponse = with_coinset_client_retries(|| async {
        client.get_coin_record_by_name(parent_coin_info).await
    })
    .await?;
    let Some(parent_record) = parent_response.coin_record else {
        return Ok(None);
    };
    if parent_record.spent_block_index == 0 {
        return Ok(None);
    }
    let parent_coin_id = parent_record.coin.coin_id();
    let spent_block_index = parent_record.spent_block_index;
    let solution_response: GetPuzzleAndSolutionResponse = with_coinset_client_retries(|| async {
        client
            .get_puzzle_and_solution(parent_coin_id, Some(spent_block_index))
            .await
    })
    .await?;
    Ok(solution_response.coin_solution)
}

pub(crate) async fn cat_from_record(
    client: &CoinsetClient,
    record: &CoinRecord,
) -> SignerResult<Option<Cat>> {
    let Some(parent_spend) = fetch_parent_coin_spend(client, record.coin.parent_coin_info).await?
    else {
        return Ok(None);
    };
    parse_cat_from_parent_spend(record.coin, &parent_spend)
}

/// Cat from parent spend.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn cat_from_parent_spend(coin: Coin, parent_spend: &CoinSpend) -> SignerResult<Option<Cat>> {
    parse_cat_from_parent_spend(coin, parent_spend)
}

/// Require cat from parent spend.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn require_cat_from_parent_spend(coin: Coin, parent_spend: &CoinSpend) -> SignerResult<Cat> {
    cat_from_parent_spend(coin, parent_spend)?.ok_or(SignerError::PresplitCoinNotFound)
}

/// One CAT child from a parent spend plus serialized inner-p2 `CREATE_COIN` memos (if any).
type CatChildWithMemos = (Cat, Option<Vec<u8>>);

fn parse_cat_children_with_memos(
    parent_coin: Coin,
    parent_spend: &CoinSpend,
) -> SignerResult<Option<Vec<CatChildWithMemos>>> {
    let puzzle_reveal = parent_spend.puzzle_reveal.clone();
    let solution = parent_spend.solution.clone();
    catch_unwind(AssertUnwindSafe(move || {
        parse_cat_children_with_memos_inner(parent_coin, &puzzle_reveal, &solution)
    }))
    .map_err(|_| unparseable_cat_lineage("clvm panic while parsing cat children"))?
}

/// Same lineage path as [`Cat::parse_children`], but retains p2 `CREATE_COIN` memo bytes.
fn parse_cat_children_with_memos_inner(
    parent_coin: Coin,
    puzzle_reveal: &[u8],
    solution: &[u8],
) -> SignerResult<Option<Vec<CatChildWithMemos>>> {
    let mut allocator = Allocator::new();
    let parent_puzzle_ptr = node_from_bytes(&mut allocator, puzzle_reveal)
        .map_err(|err| unparseable_cat_lineage(err.to_string()))?;
    let parent_solution_ptr = node_from_bytes(&mut allocator, solution)
        .map_err(|err| unparseable_cat_lineage(err.to_string()))?;
    let parent_puzzle = Puzzle::parse(&allocator, parent_puzzle_ptr);

    let Some(parent_layer) = CatLayer::<Puzzle>::parse_puzzle(&allocator, parent_puzzle)
        .map_err(|err| unparseable_cat_lineage(err.to_string()))?
    else {
        return Ok(None);
    };
    let parent_solution = CatLayer::<Puzzle>::parse_solution(&allocator, parent_solution_ptr)
        .map_err(|err| unparseable_cat_lineage(err.to_string()))?;

    let mut hidden_puzzle_hash = None;
    let mut p2_puzzle_hash = parent_layer.inner_puzzle.curried_puzzle_hash().into();
    let mut inner_spend = Spend::new(
        parent_layer.inner_puzzle.ptr(),
        parent_solution.inner_puzzle_solution,
    );
    let mut revoke = false;

    if let Some(revocation_layer) =
        RevocationLayer::parse_puzzle(&allocator, parent_layer.inner_puzzle)
            .map_err(|err| unparseable_cat_lineage(err.to_string()))?
    {
        hidden_puzzle_hash = Some(revocation_layer.hidden_puzzle_hash);
        p2_puzzle_hash = revocation_layer.inner_puzzle_hash;
        let revocation_solution =
            RevocationLayer::parse_solution(&allocator, parent_solution.inner_puzzle_solution)
                .map_err(|err| unparseable_cat_lineage(err.to_string()))?;
        inner_spend = Spend::new(revocation_solution.puzzle, revocation_solution.solution);
        revoke = revocation_solution.hidden;
    }

    let parent_cat = Cat::new(
        parent_coin,
        parent_solution.lineage_proof,
        CatInfo::new(parent_layer.asset_id, hidden_puzzle_hash, p2_puzzle_hash),
    );

    let output = run_puzzle(&mut allocator, inner_spend.puzzle, inner_spend.solution)
        .map_err(|err| unparseable_cat_lineage(err.to_string()))?;
    let conditions = Vec::<Condition<NodePtr>>::from_clvm(&allocator, output)
        .map_err(|err| unparseable_cat_lineage(err.to_string()))?;

    let mut children = Vec::new();
    for condition in conditions {
        let Some(create_coin) = condition.into_create_coin() else {
            continue;
        };
        let memos_bytes = match create_coin.memos {
            Memos::Some(ptr) => Some(
                node_to_bytes(&allocator, ptr)
                    .map_err(|err| unparseable_cat_lineage(err.to_string()))?,
            ),
            Memos::None => None,
        };
        let child = parent_cat.child_from_p2_create_coin(&allocator, create_coin, revoke);
        children.push((child, memos_bytes));
    }
    Ok(Some(children))
}

fn parse_cat_children(
    parent_coin: Coin,
    parent_spend: &CoinSpend,
) -> SignerResult<Option<Vec<Cat>>> {
    Ok(parse_cat_children_with_memos(parent_coin, parent_spend)?
        .map(|children| children.into_iter().map(|(cat, _memos)| cat).collect()))
}

fn parse_cat_from_parent_spend(coin: Coin, parent_spend: &CoinSpend) -> SignerResult<Option<Cat>> {
    Ok(
        parse_cat_children(parent_spend.coin, parent_spend)?.and_then(|children| {
            children
                .into_iter()
                .find(|cat| cat.coin.coin_id() == coin.coin_id())
        }),
    )
}

/// Child cat asset ids from parent spend.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn child_cat_asset_ids_from_parent_spend(
    parent_coin: Coin,
    parent_spend: &CoinSpend,
) -> SignerResult<HashMap<String, String>> {
    let Some(children) = parse_cat_children(parent_coin, parent_spend)? else {
        return Ok(HashMap::new());
    };
    Ok(children
        .into_iter()
        .map(|cat| {
            (
                hex::encode(cat.coin.coin_id()),
                normalize_hex_id(&hex::encode(cat.info.asset_id)),
            )
        })
        .collect())
}

/// CAT child plus serialized p2 `CREATE_COIN` memos from the parent spend.
///
/// Uses the single local CAT-children parse (inner p2 `CREATE_COIN`, same semantics as
/// [`Cat::parse_children`]). Memo bytes are the full memos list when present.
///
/// # Errors
///
/// Returns an error when CAT lineage parsing fails.
pub fn cat_child_p2_create_coin_memos(
    parent_spend: &CoinSpend,
    child_coin_id: Bytes32,
) -> SignerResult<Option<CatChildWithMemos>> {
    Ok(
        parse_cat_children_with_memos(parent_spend.coin, parent_spend)?.and_then(|children| {
            children
                .into_iter()
                .find(|(cat, _)| cat.coin.coin_id() == child_coin_id)
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::{Bytes, Coin, CoinSpend};

    fn empty_parent_spend() -> CoinSpend {
        CoinSpend {
            coin: Coin::new(
                chia_protocol::Bytes32::default(),
                chia_protocol::Bytes32::default(),
                0,
            ),
            puzzle_reveal: Bytes::new(vec![]).into(),
            solution: Bytes::new(vec![]).into(),
        }
    }

    #[test]
    fn parse_cat_children_empty_spend_returns_unparseable_lineage_not_panic() {
        let _parent = Coin::new(
            chia_protocol::Bytes32::new([1; 32]),
            chia_protocol::Bytes32::new([2; 32]),
            1,
        );
        let child = Coin::new(
            chia_protocol::Bytes32::new([3; 32]),
            chia_protocol::Bytes32::new([4; 32]),
            1,
        );
        let err = parse_cat_from_parent_spend(child, &empty_parent_spend()).expect_err("parse");
        assert!(matches!(err, SignerError::UnparseableCatLineage(_)));
    }
}

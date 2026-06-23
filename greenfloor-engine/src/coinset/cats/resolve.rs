use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use chia_protocol::{Coin, CoinSpend};
use chia_sdk_coinset::{
    ChiaRpcClient, CoinRecord, CoinsetClient, GetCoinRecordResponse, GetPuzzleAndSolutionResponse,
};
use chia_sdk_driver::{Cat, Puzzle};
use clvmr::{serde::node_from_bytes, Allocator};

use crate::coinset::retry::with_coinset_client_retries;
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;

fn unparseable_cat_lineage(detail: impl Into<String>) -> SignerError {
    SignerError::UnparseableCatLineage(detail.into())
}

pub(crate) async fn cat_from_record(
    client: &CoinsetClient,
    record: &CoinRecord,
) -> SignerResult<Option<Cat>> {
    let parent_response: GetCoinRecordResponse = with_coinset_client_retries(|| async {
        client
            .get_coin_record_by_name(record.coin.parent_coin_info)
            .await
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
    let Some(parent_spend) = solution_response.coin_solution else {
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

fn parse_cat_children(
    parent_coin: Coin,
    parent_spend: &CoinSpend,
) -> SignerResult<Option<Vec<Cat>>> {
    let puzzle_reveal = parent_spend.puzzle_reveal.clone();
    let solution = parent_spend.solution.clone();
    catch_unwind(AssertUnwindSafe(move || {
        parse_cat_children_inner(parent_coin, &puzzle_reveal, &solution)
    }))
    .map_err(|_| unparseable_cat_lineage("clvm panic while parsing cat children"))?
}

fn parse_cat_children_inner(
    parent_coin: Coin,
    puzzle_reveal: &[u8],
    solution: &[u8],
) -> SignerResult<Option<Vec<Cat>>> {
    let mut allocator = Allocator::new();
    let parent_puzzle_ptr = node_from_bytes(&mut allocator, puzzle_reveal)
        .map_err(|err| unparseable_cat_lineage(err.to_string()))?;
    let parent_solution_ptr = node_from_bytes(&mut allocator, solution)
        .map_err(|err| unparseable_cat_lineage(err.to_string()))?;
    let parent_puzzle = Puzzle::parse(&allocator, parent_puzzle_ptr);
    Cat::parse_children(
        &mut allocator,
        parent_coin,
        parent_puzzle,
        parent_solution_ptr,
    )
    .map_err(|err| unparseable_cat_lineage(err.to_string()))
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

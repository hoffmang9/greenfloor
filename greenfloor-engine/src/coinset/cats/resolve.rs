use std::collections::HashMap;

use chia_protocol::{Coin, CoinSpend};
use chia_sdk_coinset::{
    ChiaRpcClient, CoinRecord, CoinsetClient, GetCoinRecordResponse, GetPuzzleAndSolutionResponse,
};
use chia_sdk_driver::{Cat, Puzzle};
use clvmr::{serde::node_from_bytes, Allocator};

use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;

pub(crate) async fn cat_from_record(
    client: &CoinsetClient,
    record: &CoinRecord,
) -> SignerResult<Option<Cat>> {
    let parent_response: GetCoinRecordResponse = client
        .get_coin_record_by_name(record.coin.parent_coin_info)
        .await
        .map_err(SignerError::from)?;
    let Some(parent_record) = parent_response.coin_record else {
        return Ok(None);
    };
    if parent_record.spent_block_index == 0 {
        return Ok(None);
    }
    let solution_response: GetPuzzleAndSolutionResponse = client
        .get_puzzle_and_solution(
            parent_record.coin.coin_id(),
            Some(parent_record.spent_block_index),
        )
        .await
        .map_err(SignerError::from)?;
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
    let mut allocator = Allocator::new();
    let parent_puzzle_ptr = node_from_bytes(&mut allocator, parent_spend.puzzle_reveal.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let parent_solution_ptr = node_from_bytes(&mut allocator, parent_spend.solution.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let parent_puzzle = Puzzle::parse(&allocator, parent_puzzle_ptr);
    Cat::parse_children(
        &mut allocator,
        parent_coin,
        parent_puzzle,
        parent_solution_ptr,
    )
    .map_err(|err| SignerError::Driver(err.to_string()))
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

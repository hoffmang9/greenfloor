use chia_protocol::{Coin, CoinSpend, SpendBundle};
use chia_sdk_driver::{Cat, Puzzle};
use clvmr::{serde::node_from_bytes, Allocator, NodePtr};

use crate::error::{SignerError, SignerResult};

pub(super) struct ParsedOfferMakerSpend {
    pub cat: Option<Cat>,
    pub inner_puzzle: Puzzle,
    pub inner_solution: NodePtr,
}

pub(super) fn binding_parse_err(detail: impl Into<String>) -> SignerError {
    SignerError::OfferCancelPresplitBindingParseFailed {
        detail: detail.into(),
    }
}

pub(super) fn coin_spend_for_presplit_input(
    spend_bundle: &SpendBundle,
    coin: Coin,
) -> SignerResult<&CoinSpend> {
    for coin_spend in &spend_bundle.coin_spends {
        if coin_spend.coin.coin_id() == coin.coin_id() {
            return Ok(coin_spend);
        }
    }
    Err(SignerError::OfferCancelNoSpendableInput)
}

pub(super) fn parse_offer_maker_coin_spend(
    allocator: &mut Allocator,
    coin: Coin,
    coin_spend: &CoinSpend,
) -> SignerResult<ParsedOfferMakerSpend> {
    let puzzle_ptr = node_from_bytes(allocator, coin_spend.puzzle_reveal.as_ref())
        .map_err(|err| binding_parse_err(err.to_string()))?;
    let puzzle = Puzzle::parse(allocator, puzzle_ptr);
    let solution_ptr = node_from_bytes(allocator, coin_spend.solution.as_ref())
        .map_err(|err| binding_parse_err(err.to_string()))?;
    if let Some((parsed_cat, inner_puzzle, inner_solution)) =
        Cat::parse(allocator, coin_spend.coin, puzzle, solution_ptr).map_err(SignerError::from)?
    {
        if parsed_cat.coin.coin_id() != coin.coin_id() {
            return Err(SignerError::OfferCancelNoSpendableInput);
        }
        Ok(ParsedOfferMakerSpend {
            cat: Some(parsed_cat),
            inner_puzzle,
            inner_solution,
        })
    } else {
        Ok(ParsedOfferMakerSpend {
            cat: None,
            inner_puzzle: puzzle,
            inner_solution: solution_ptr,
        })
    }
}

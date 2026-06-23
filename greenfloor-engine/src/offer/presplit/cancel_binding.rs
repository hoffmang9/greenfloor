//! Parse presplit offer input spends to recover cancel/reclaim binding hashes.

use chia_protocol::{Bytes32, Coin, CoinSpend, SpendBundle};
use chia_puzzles::{DELEGATED_PUZZLE_FEEDER_HASH, ONE_OF_N_HASH};
use chia_sdk_driver::{Cat, Puzzle};
use chia_sdk_types::puzzles::{
    DelegatedPuzzleFeederArgs, DelegatedPuzzleFeederSolution, IndexWrapperArgs, OneOfNArgs,
    OneOfNSolution, INDEX_WRAPPER_HASH,
};
use clvm_traits::FromClvm;
use clvm_utils::{tree_hash, TreeHash};
use clvmr::{serde::node_from_bytes, Allocator, NodePtr};

use crate::error::{SignerError, SignerResult};
use crate::vault::members::p2_conditions_or_singleton_puzzle_hash;

/// Presplit maker binding recovered from an offer input spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PresplitCoinBinding {
    pub fixed_conditions_tree_hash: TreeHash,
    pub binding_p2_puzzle_hash: Bytes32,
    /// Set when the maker input spend is a CAT (presplit CAT cancel must use `Cat::spend_all`).
    pub parsed_cat: Option<Cat>,
}

/// Result of attempting to read presplit binding from a cancellable maker input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum PresplitBindingLookup {
    Found(PresplitCoinBinding),
    /// Maker spend is not a presplit `P2_CONDITIONS_OR_SINGLETON` input.
    NotPresplitMaker,
}

#[derive(Debug)]
enum PeelError {
    NotPresplitLayout,
    Parse(SignerError),
}

struct ParsedOfferMakerSpend {
    cat: Option<Cat>,
    inner_puzzle: Puzzle,
    inner_solution: NodePtr,
}

fn binding_parse_err(detail: impl Into<String>) -> SignerError {
    SignerError::OfferCancelPresplitBindingParseFailed {
        detail: detail.into(),
    }
}

fn coin_spend_for_presplit_input(
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

fn parse_offer_maker_coin_spend(
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

fn try_peel_puzzle_wrapper(
    allocator: &Allocator,
    puzzle: Puzzle,
) -> Result<Option<Puzzle>, PeelError> {
    let Some(curried) = puzzle.as_curried() else {
        return Ok(None);
    };
    if curried.mod_hash == INDEX_WRAPPER_HASH {
        let args = IndexWrapperArgs::<NodePtr, NodePtr>::from_clvm(allocator, curried.args)
            .map_err(|err| PeelError::Parse(binding_parse_err(err.to_string())))?;
        return Ok(Some(Puzzle::parse(allocator, args.inner_puzzle)));
    }
    if curried.mod_hash == DELEGATED_PUZZLE_FEEDER_HASH.into() {
        let args = DelegatedPuzzleFeederArgs::<NodePtr>::from_clvm(allocator, curried.args)
            .map_err(|err| PeelError::Parse(binding_parse_err(err.to_string())))?;
        return Ok(Some(Puzzle::parse(allocator, args.inner_puzzle)));
    }
    Ok(None)
}

fn peel_index_wrapper_puzzle(
    allocator: &Allocator,
    mut puzzle: Puzzle,
) -> Result<NodePtr, PeelError> {
    for _ in 0..8 {
        match try_peel_puzzle_wrapper(allocator, puzzle)? {
            Some(inner) => puzzle = inner,
            None => return Ok(puzzle.ptr()),
        }
    }
    Err(PeelError::Parse(binding_parse_err(
        "presplit fixed member puzzle wrapper depth exceeded",
    )))
}

fn peel_to_one_of_n_puzzle(
    allocator: &Allocator,
    mut puzzle: Puzzle,
) -> Result<(NodePtr, NodePtr), PeelError> {
    for _ in 0..8 {
        let Some(curried) = puzzle.as_curried() else {
            return Err(PeelError::NotPresplitLayout);
        };
        if curried.mod_hash == ONE_OF_N_HASH.into() {
            return Ok((curried.curried_ptr, curried.args));
        }
        puzzle = match try_peel_puzzle_wrapper(allocator, puzzle)? {
            Some(inner) => inner,
            None => return Err(PeelError::NotPresplitLayout),
        };
    }
    Err(PeelError::Parse(binding_parse_err(
        "presplit input inner puzzle wrapper depth exceeded",
    )))
}

fn peel_to_one_of_n_solution(
    allocator: &Allocator,
    mut solution: NodePtr,
) -> Result<NodePtr, PeelError> {
    for _ in 0..8 {
        if OneOfNSolution::<NodePtr, NodePtr>::from_clvm(allocator, solution).is_ok() {
            return Ok(solution);
        }
        if let Ok(feeder) = DelegatedPuzzleFeederSolution::<NodePtr, NodePtr, NodePtr>::from_clvm(
            allocator, solution,
        ) {
            solution = feeder.inner_solution;
            continue;
        }
        return Ok(solution);
    }
    Err(PeelError::Parse(binding_parse_err(
        "presplit input inner solution wrapper depth exceeded",
    )))
}

fn presplit_fixed_delegated_puzzle_hash_from_inner(
    allocator: &Allocator,
    inner_puzzle: Puzzle,
    inner_solution: NodePtr,
) -> Result<TreeHash, PeelError> {
    let (_one_of_n_puzzle, one_of_n_args_ptr) = peel_to_one_of_n_puzzle(allocator, inner_puzzle)?;
    let one_of_n_solution_ptr = peel_to_one_of_n_solution(allocator, inner_solution)?;
    let _one_of_n_args = OneOfNArgs::from_clvm(allocator, one_of_n_args_ptr)
        .map_err(|err| PeelError::Parse(binding_parse_err(err.to_string())))?;
    let one_of_n_solution =
        OneOfNSolution::<NodePtr, NodePtr>::from_clvm(allocator, one_of_n_solution_ptr)
            .map_err(|err| PeelError::Parse(binding_parse_err(err.to_string())))?;
    let member_puzzle = Puzzle::parse(allocator, one_of_n_solution.member_puzzle);
    let fixed_delegated_puzzle_ptr = peel_index_wrapper_puzzle(allocator, member_puzzle)?;
    Ok(tree_hash(allocator, fixed_delegated_puzzle_ptr))
}

/// Verify a stored fixed delegated hash matches the presplit binding p2 puzzle hash.
///
/// For presplit CAT maker coins, pass the CAT inner p2 hash (`cat.info.p2_puzzle_hash`).
/// For presplit XCH maker coins, pass `coin.puzzle_hash`.
///
/// # Errors
///
/// Returns an error when the hash does not match the binding.
pub(crate) fn verify_fixed_delegated_puzzle_hash_for_binding(
    launcher_id: Bytes32,
    binding_p2_puzzle_hash: Bytes32,
    fixed_delegated_puzzle_hash: TreeHash,
) -> SignerResult<()> {
    let expected =
        p2_conditions_or_singleton_puzzle_hash(fixed_delegated_puzzle_hash, launcher_id)?;
    if binding_p2_puzzle_hash != expected.puzzle_hash.into() {
        return Err(SignerError::PresplitCoinPuzzleHashMismatch);
    }
    Ok(())
}

fn presplit_binding_from_parsed_spend(
    allocator: &Allocator,
    launcher_id: Bytes32,
    coin: Coin,
    parsed: &ParsedOfferMakerSpend,
) -> Result<PresplitBindingLookup, SignerError> {
    let fixed_conditions_tree_hash = match presplit_fixed_delegated_puzzle_hash_from_inner(
        allocator,
        parsed.inner_puzzle,
        parsed.inner_solution,
    ) {
        Ok(hash) => hash,
        Err(PeelError::NotPresplitLayout) => return Ok(PresplitBindingLookup::NotPresplitMaker),
        Err(PeelError::Parse(err)) => return Err(err),
    };
    let binding_p2_puzzle_hash = parsed
        .cat
        .map_or(coin.puzzle_hash, |value| value.info.p2_puzzle_hash);
    verify_fixed_delegated_puzzle_hash_for_binding(
        launcher_id,
        binding_p2_puzzle_hash,
        fixed_conditions_tree_hash,
    )?;
    Ok(PresplitBindingLookup::Found(PresplitCoinBinding {
        fixed_conditions_tree_hash,
        binding_p2_puzzle_hash,
        parsed_cat: parsed.cat,
    }))
}

fn presplit_binding_from_coin_spend(
    launcher_id: Bytes32,
    coin: Coin,
    coin_spend: &CoinSpend,
) -> Result<PresplitBindingLookup, SignerError> {
    let mut allocator = Allocator::new();
    let parsed = parse_offer_maker_coin_spend(&mut allocator, coin, coin_spend)?;
    presplit_binding_from_parsed_spend(&allocator, launcher_id, coin, &parsed)
}

/// Read presplit maker binding from a cancellable input (XCH or CAT).
///
/// # Errors
///
/// Returns an error if the input spend cannot be read or presplit verification fails for this
/// vault launcher.
pub(crate) fn presplit_binding_from_coin_input(
    launcher_id: Bytes32,
    coin: Coin,
    spend_bundle: &SpendBundle,
) -> SignerResult<PresplitBindingLookup> {
    let coin_spend = coin_spend_for_presplit_input(spend_bundle, coin)?;
    presplit_binding_from_coin_spend(launcher_id, coin, coin_spend)
}

/// Parse a vault CAT maker input from the offer file without presplit binding checks.
///
/// # Errors
///
/// Returns an error if the offer input spend cannot be read.
pub(crate) fn offer_maker_cat_from_coin_input(
    coin: Coin,
    spend_bundle: &SpendBundle,
) -> SignerResult<Option<Cat>> {
    let coin_spend = coin_spend_for_presplit_input(spend_bundle, coin)?;
    let mut allocator = Allocator::new();
    Ok(parse_offer_maker_coin_spend(&mut allocator, coin, coin_spend)?.cat)
}

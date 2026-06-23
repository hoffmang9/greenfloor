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

fn peel_index_wrapper_puzzle(allocator: &Allocator, mut puzzle: Puzzle) -> SignerResult<NodePtr> {
    for _ in 0..8 {
        let Some(curried) = puzzle.as_curried() else {
            return Ok(puzzle.ptr());
        };
        if curried.mod_hash == INDEX_WRAPPER_HASH {
            let args = IndexWrapperArgs::<NodePtr, NodePtr>::from_clvm(allocator, curried.args)
                .map_err(|err| binding_parse_err(err.to_string()))?;
            puzzle = Puzzle::parse(allocator, args.inner_puzzle);
            continue;
        }
        if curried.mod_hash == DELEGATED_PUZZLE_FEEDER_HASH.into() {
            let args = DelegatedPuzzleFeederArgs::<NodePtr>::from_clvm(allocator, curried.args)
                .map_err(|err| binding_parse_err(err.to_string()))?;
            puzzle = Puzzle::parse(allocator, args.inner_puzzle);
            continue;
        }
        return Ok(puzzle.ptr());
    }
    Err(binding_parse_err(
        "presplit fixed member puzzle wrapper depth exceeded",
    ))
}

fn peel_to_one_of_n_puzzle(
    allocator: &Allocator,
    mut puzzle: Puzzle,
) -> SignerResult<(NodePtr, NodePtr)> {
    for _ in 0..8 {
        let Some(curried) = puzzle.as_curried() else {
            return Err(binding_parse_err(
                "presplit input inner puzzle is not curried",
            ));
        };
        if curried.mod_hash == ONE_OF_N_HASH.into() {
            return Ok((curried.curried_ptr, curried.args));
        }
        if curried.mod_hash == INDEX_WRAPPER_HASH {
            let args = IndexWrapperArgs::<NodePtr, NodePtr>::from_clvm(allocator, curried.args)
                .map_err(|err| binding_parse_err(err.to_string()))?;
            puzzle = Puzzle::parse(allocator, args.inner_puzzle);
            continue;
        }
        if curried.mod_hash == DELEGATED_PUZZLE_FEEDER_HASH.into() {
            let args = DelegatedPuzzleFeederArgs::<NodePtr>::from_clvm(allocator, curried.args)
                .map_err(|err| binding_parse_err(err.to_string()))?;
            puzzle = Puzzle::parse(allocator, args.inner_puzzle);
            continue;
        }
        return Err(binding_parse_err(
            "presplit input inner puzzle is not one-of-n",
        ));
    }
    Err(binding_parse_err(
        "presplit input inner puzzle wrapper depth exceeded",
    ))
}

fn peel_to_one_of_n_solution(
    allocator: &Allocator,
    mut solution: NodePtr,
) -> SignerResult<NodePtr> {
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
    Err(binding_parse_err(
        "presplit input inner solution wrapper depth exceeded",
    ))
}

fn presplit_fixed_delegated_puzzle_hash_from_inner(
    allocator: &Allocator,
    inner_puzzle: Puzzle,
    inner_solution: NodePtr,
) -> SignerResult<TreeHash> {
    let (_one_of_n_puzzle, one_of_n_args_ptr) = peel_to_one_of_n_puzzle(allocator, inner_puzzle)?;
    let one_of_n_solution_ptr = peel_to_one_of_n_solution(allocator, inner_solution)?;
    let _one_of_n_args = OneOfNArgs::from_clvm(allocator, one_of_n_args_ptr)
        .map_err(|err| binding_parse_err(err.to_string()))?;
    let one_of_n_solution =
        OneOfNSolution::<NodePtr, NodePtr>::from_clvm(allocator, one_of_n_solution_ptr)
            .map_err(|err| binding_parse_err(err.to_string()))?;
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

fn presplit_binding_from_coin_spend(
    launcher_id: Bytes32,
    coin: Coin,
    coin_spend: &CoinSpend,
) -> SignerResult<PresplitCoinBinding> {
    let mut allocator = Allocator::new();
    let puzzle_ptr = node_from_bytes(&mut allocator, coin_spend.puzzle_reveal.as_ref())
        .map_err(|err| binding_parse_err(err.to_string()))?;
    let puzzle = Puzzle::parse(&allocator, puzzle_ptr);
    let solution_ptr = node_from_bytes(&mut allocator, coin_spend.solution.as_ref())
        .map_err(|err| binding_parse_err(err.to_string()))?;
    let (fixed_conditions_tree_hash, binding_p2_puzzle_hash, parsed_cat) =
        if let Some((parsed_cat, inner_puzzle, inner_solution)) =
            Cat::parse(&allocator, coin_spend.coin, puzzle, solution_ptr)
                .map_err(SignerError::from)?
        {
            if parsed_cat.coin.coin_id() != coin.coin_id() {
                return Err(SignerError::OfferCancelNoSpendableInput);
            }
            let hash = presplit_fixed_delegated_puzzle_hash_from_inner(
                &allocator,
                inner_puzzle,
                inner_solution,
            )?;
            (hash, parsed_cat.info.p2_puzzle_hash, Some(parsed_cat))
        } else {
            let hash =
                presplit_fixed_delegated_puzzle_hash_from_inner(&allocator, puzzle, solution_ptr)?;
            (hash, coin.puzzle_hash, None)
        };
    verify_fixed_delegated_puzzle_hash_for_binding(
        launcher_id,
        binding_p2_puzzle_hash,
        fixed_conditions_tree_hash,
    )?;
    Ok(PresplitCoinBinding {
        fixed_conditions_tree_hash,
        binding_p2_puzzle_hash,
        parsed_cat,
    })
}

/// Read presplit maker binding from a cancellable input (XCH or CAT).
///
/// # Errors
///
/// Returns an error if the input spend or puzzle structure cannot be parsed.
pub(crate) fn presplit_binding_from_coin_input(
    launcher_id: Bytes32,
    coin: Coin,
    spend_bundle: &SpendBundle,
) -> SignerResult<PresplitCoinBinding> {
    let coin_spend = coin_spend_for_presplit_input(spend_bundle, coin)?;
    presplit_binding_from_coin_spend(launcher_id, coin, coin_spend)
}

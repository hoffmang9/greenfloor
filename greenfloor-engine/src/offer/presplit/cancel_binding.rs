//! Parse presplit offer input spends to recover cancel/reclaim binding hashes.

use chia_protocol::{Bytes32, CoinSpend, SpendBundle};
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

fn coin_spend_for_presplit_cat_input<'a>(
    spend_bundle: &'a SpendBundle,
    chain_cat: &Cat,
) -> SignerResult<&'a CoinSpend> {
    let target_p2 = chain_cat.info.p2_puzzle_hash;
    for coin_spend in &spend_bundle.coin_spends {
        let mut allocator = Allocator::new();
        let puzzle_ptr = node_from_bytes(&mut allocator, coin_spend.puzzle_reveal.as_ref())
            .map_err(|err| SignerError::Driver(err.to_string()))?;
        let puzzle = Puzzle::parse(&allocator, puzzle_ptr);
        let solution_ptr = node_from_bytes(&mut allocator, coin_spend.solution.as_ref())
            .map_err(|err| SignerError::Driver(err.to_string()))?;
        let Some((parsed_cat, ..)) = Cat::parse(&allocator, coin_spend.coin, puzzle, solution_ptr)
            .map_err(SignerError::from)?
        else {
            continue;
        };
        if parsed_cat.info.p2_puzzle_hash == target_p2 {
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
                .map_err(|err| SignerError::Driver(err.to_string()))?;
            puzzle = Puzzle::parse(allocator, args.inner_puzzle);
            continue;
        }
        if curried.mod_hash == DELEGATED_PUZZLE_FEEDER_HASH.into() {
            let args = DelegatedPuzzleFeederArgs::<NodePtr>::from_clvm(allocator, curried.args)
                .map_err(|err| SignerError::Driver(err.to_string()))?;
            puzzle = Puzzle::parse(allocator, args.inner_puzzle);
            continue;
        }
        return Ok(puzzle.ptr());
    }
    Err(SignerError::Driver(
        "presplit fixed member puzzle wrapper depth exceeded".to_string(),
    ))
}

fn peel_to_one_of_n_puzzle(
    allocator: &Allocator,
    mut puzzle: Puzzle,
) -> SignerResult<(NodePtr, NodePtr)> {
    for _ in 0..8 {
        let Some(curried) = puzzle.as_curried() else {
            return Err(SignerError::Driver(
                "presplit input inner puzzle is not curried".to_string(),
            ));
        };
        if curried.mod_hash == ONE_OF_N_HASH.into() {
            return Ok((curried.curried_ptr, curried.args));
        }
        if curried.mod_hash == INDEX_WRAPPER_HASH {
            let args = IndexWrapperArgs::<NodePtr, NodePtr>::from_clvm(allocator, curried.args)
                .map_err(|err| SignerError::Driver(err.to_string()))?;
            puzzle = Puzzle::parse(allocator, args.inner_puzzle);
            continue;
        }
        if curried.mod_hash == DELEGATED_PUZZLE_FEEDER_HASH.into() {
            let args = DelegatedPuzzleFeederArgs::<NodePtr>::from_clvm(allocator, curried.args)
                .map_err(|err| SignerError::Driver(err.to_string()))?;
            puzzle = Puzzle::parse(allocator, args.inner_puzzle);
            continue;
        }
        return Err(SignerError::Driver(
            "presplit input inner puzzle is not one-of-n".to_string(),
        ));
    }
    Err(SignerError::Driver(
        "presplit input inner puzzle wrapper depth exceeded".to_string(),
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
    Err(SignerError::Driver(
        "presplit input inner solution wrapper depth exceeded".to_string(),
    ))
}

/// Verify a stored fixed delegated hash matches the on-chain presplit CAT puzzle hash.
///
/// # Errors
///
/// Returns an error when the hash does not match the CAT binding.
pub(crate) fn verify_fixed_delegated_puzzle_hash_for_cat(
    launcher_id: Bytes32,
    chain_cat: &Cat,
    fixed_delegated_puzzle_hash: TreeHash,
) -> SignerResult<()> {
    let expected =
        p2_conditions_or_singleton_puzzle_hash(fixed_delegated_puzzle_hash, launcher_id)?;
    if chain_cat.info.p2_puzzle_hash != expected.puzzle_hash.into() {
        return Err(SignerError::PresplitCoinPuzzleHashMismatch);
    }
    Ok(())
}

/// Read the fixed-conditions member hash from the presplit offer input coin spend.
///
/// The offer file embeds the maker's input spend; the fixed branch delegated puzzle tree hash
/// deterministically yields the member hash used by cancel/reclaim spends.
///
/// # Errors
///
/// Returns an error if the input spend or puzzle structure cannot be parsed.
pub(crate) fn presplit_fixed_conditions_tree_hash_from_input(
    launcher_id: Bytes32,
    chain_cat: &Cat,
    spend_bundle: &SpendBundle,
) -> SignerResult<TreeHash> {
    let coin_spend = coin_spend_for_presplit_cat_input(spend_bundle, chain_cat)?;
    let mut allocator = Allocator::new();
    let puzzle_ptr = node_from_bytes(&mut allocator, coin_spend.puzzle_reveal.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let puzzle = Puzzle::parse(&allocator, puzzle_ptr);
    let solution_ptr = node_from_bytes(&mut allocator, coin_spend.solution.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let Some((_, inner_puzzle, inner_solution)) =
        Cat::parse(&allocator, coin_spend.coin, puzzle, solution_ptr).map_err(SignerError::from)?
    else {
        return Err(SignerError::OfferCancelNoSpendableInput);
    };
    let (_one_of_n_puzzle, one_of_n_args_ptr) = peel_to_one_of_n_puzzle(&allocator, inner_puzzle)?;
    let one_of_n_solution_ptr = peel_to_one_of_n_solution(&allocator, inner_solution)?;
    let _one_of_n_args = OneOfNArgs::from_clvm(&allocator, one_of_n_args_ptr)
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let one_of_n_solution =
        OneOfNSolution::<NodePtr, NodePtr>::from_clvm(&allocator, one_of_n_solution_ptr)
            .map_err(|err| SignerError::Driver(err.to_string()))?;
    let member_puzzle = Puzzle::parse(&allocator, one_of_n_solution.member_puzzle);
    let fixed_delegated_puzzle_ptr = peel_index_wrapper_puzzle(&allocator, member_puzzle)?;
    let fixed_delegated_puzzle_hash = tree_hash(&allocator, fixed_delegated_puzzle_ptr);
    verify_fixed_delegated_puzzle_hash_for_cat(
        launcher_id,
        chain_cat,
        fixed_delegated_puzzle_hash,
    )?;
    Ok(fixed_delegated_puzzle_hash)
}

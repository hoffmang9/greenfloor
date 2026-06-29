use chia_puzzles::{DELEGATED_PUZZLE_FEEDER_HASH, ONE_OF_N_HASH};
use chia_sdk_driver::Puzzle;
use chia_sdk_types::puzzles::{
    DelegatedPuzzleFeederArgs, DelegatedPuzzleFeederSolution, IndexWrapperArgs, OneOfNArgs,
    OneOfNSolution, INDEX_WRAPPER_HASH,
};
use clvm_traits::FromClvm;
use clvm_utils::{tree_hash, TreeHash};
use clvmr::{Allocator, NodePtr};

use super::parse::binding_parse_err;
use crate::error::SignerError;

#[derive(Debug)]
pub(super) enum PeelError {
    NotPresplitLayout,
    Parse(SignerError),
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

pub(super) fn presplit_fixed_delegated_puzzle_hash_from_inner(
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

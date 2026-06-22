use chia_protocol::Bytes32;
use chia_puzzles::PREVENT_MULTIPLE_CREATE_COINS_HASH;
use chia_sdk_driver::{Restriction, RestrictionKind};
use chia_sdk_types::{
    puzzles::{Force1of2RestrictedVariable, PreventConditionOpcode, Timelock},
    Mod,
};

use crate::error::SignerResult;

use super::hash::u32_to_usize;

#[must_use]
pub fn timelock_restriction(timelock: u64) -> Restriction {
    Restriction {
        kind: RestrictionKind::MemberCondition,
        puzzle_hash: Timelock::new(timelock).curry_tree_hash(),
    }
}

/// Force 1 of 2 restriction.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn force_1_of_2_restriction(
    left_side_subtree_hash: Bytes32,
    nonce: u32,
    member_validator_list_hash: Bytes32,
    delegated_puzzle_validator_list_hash: Bytes32,
) -> SignerResult<Restriction> {
    Ok(Restriction {
        kind: RestrictionKind::DelegatedPuzzleWrapper,
        puzzle_hash: Force1of2RestrictedVariable::new(
            left_side_subtree_hash,
            u32_to_usize(nonce)?,
            member_validator_list_hash,
            delegated_puzzle_validator_list_hash,
        )
        .curry_tree_hash(),
    })
}

#[must_use]
pub fn prevent_vault_side_effects_restriction() -> Vec<Restriction> {
    vec![
        prevent_condition_opcode_restriction(60),
        prevent_condition_opcode_restriction(62),
        prevent_condition_opcode_restriction(66),
        prevent_condition_opcode_restriction(67),
        prevent_multiple_create_coins_restriction(),
    ]
}

fn prevent_condition_opcode_restriction(condition_opcode: u16) -> Restriction {
    Restriction {
        kind: RestrictionKind::DelegatedPuzzleWrapper,
        puzzle_hash: PreventConditionOpcode::new(condition_opcode).curry_tree_hash(),
    }
}

fn prevent_multiple_create_coins_restriction() -> Restriction {
    Restriction {
        kind: RestrictionKind::DelegatedPuzzleWrapper,
        puzzle_hash: PREVENT_MULTIPLE_CREATE_COINS_HASH.into(),
    }
}

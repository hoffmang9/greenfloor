use chia_protocol::Bytes32;
use clvm_utils::TreeHash;

use crate::error::SignerResult;

use super::config::{MemberConfig, P2ConditionsOrSingletonHashes};
use super::curves::singleton_member_hash;
use super::hash::{m_of_n_hash, member_hash};

/// P2 conditions or singleton puzzle hash from a raw delegated CONDITIONS tree hash.
///
/// Operator state persists this raw hash as `fixed_delegated_puzzle_hash`.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn p2_conditions_or_singleton_puzzle_hash(
    fixed_delegated_puzzle_hash: TreeHash,
    launcher_id: Bytes32,
) -> SignerResult<P2ConditionsOrSingletonHashes> {
    let member_config = MemberConfig::default();
    let fixed_conditions_hash = member_hash(&member_config, fixed_delegated_puzzle_hash)?;
    p2_conditions_or_singleton_from_member_fixed(fixed_conditions_hash, launcher_id)
}

/// P2 conditions or singleton puzzle hash from an already member-wrapped fixed hash.
///
/// Cloud Wallet / ent-wallet persist this as `fixedConditionsHash` (not the raw delegated
/// CONDITIONS tree hash).
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn p2_conditions_or_singleton_from_member_fixed(
    fixed_conditions_member_hash: TreeHash,
    launcher_id: Bytes32,
) -> SignerResult<P2ConditionsOrSingletonHashes> {
    let member_config = MemberConfig::default();
    let p2_singleton_hash = singleton_member_hash(&member_config, launcher_id, false)?;
    let puzzle_hash = m_of_n_hash(
        &member_config.with_top_level(true),
        1,
        vec![fixed_conditions_member_hash, p2_singleton_hash],
    )?;
    Ok(P2ConditionsOrSingletonHashes {
        puzzle_hash,
        fixed_conditions_hash: fixed_conditions_member_hash,
        p2_singleton_hash,
    })
}

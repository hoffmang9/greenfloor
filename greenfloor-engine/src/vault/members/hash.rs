use chia_sdk_driver::{mips_puzzle_hash, MofN};
use clvm_utils::TreeHash;

use crate::error::{SignerError, SignerResult};

use super::config::MemberConfig;

pub(crate) fn u32_to_usize(value: u32) -> SignerResult<usize> {
    usize::try_from(value).map_err(|_| SignerError::UnsupportedVaultThreshold)
}

pub(crate) fn member_hash(config: &MemberConfig, inner_hash: TreeHash) -> SignerResult<TreeHash> {
    Ok(mips_puzzle_hash(
        u32_to_usize(config.nonce)?,
        config.restrictions.clone(),
        inner_hash,
        config.top_level,
    ))
}

/// M of n hash.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn m_of_n_hash(
    config: &MemberConfig,
    required: u32,
    items: Vec<TreeHash>,
) -> SignerResult<TreeHash> {
    let required_usize = u32_to_usize(required)?;
    member_hash(config, MofN::new(required_usize, items).inner_puzzle_hash())
}

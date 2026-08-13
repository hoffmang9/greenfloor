//! Parse presplit offer input spends to recover cancel/reclaim binding hashes.

mod parse;
mod peel;

use chia_protocol::{Bytes32, Coin, SpendBundle};
use chia_sdk_driver::Cat;
use clvm_utils::TreeHash;
use clvmr::Allocator;

use crate::error::{OfferError, SignerError, SignerResult};
use crate::vault::members::{
    p2_conditions_or_singleton_from_member_fixed, p2_conditions_or_singleton_puzzle_hash,
};

use parse::{coin_spend_for_presplit_input, parse_offer_maker_coin_spend, ParsedOfferMakerSpend};
use peel::{presplit_fixed_delegated_puzzle_hash_from_inner, PeelError};

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
    coin_spend: &chia_protocol::CoinSpend,
) -> Result<PresplitBindingLookup, SignerError> {
    let mut allocator = Allocator::new();
    let parsed = parse_offer_maker_coin_spend(&mut allocator, coin, coin_spend)?;
    presplit_binding_from_parsed_spend(&allocator, launcher_id, coin, &parsed)
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
        return Err(SignerError::Offer(
            OfferError::PresplitCoinPuzzleHashMismatch,
        ));
    }
    Ok(())
}

/// Resolve a stored fixed hash to the member-wrapped form used by cancel/reclaim.
///
/// Accepts operator `fixed_delegated_puzzle_hash` (raw delegated CONDITIONS tree hash) or
/// Cloud Wallet `fixedConditionsHash` (already member-wrapped). Construction errors propagate;
/// only a successful hash build with a non-matching binding p2 falls through to the other
/// encoding before returning [`crate::error::OfferError::PresplitCoinPuzzleHashMismatch`].
///
/// # Errors
///
/// Returns an error when hash construction fails or neither encoding matches the binding.
pub(crate) fn resolve_member_fixed_conditions_hash_for_binding(
    launcher_id: Bytes32,
    binding_p2_puzzle_hash: Bytes32,
    stored_hash: TreeHash,
) -> SignerResult<TreeHash> {
    let delegated_hashes = p2_conditions_or_singleton_puzzle_hash(stored_hash, launcher_id)?;
    if binding_p2_puzzle_hash == delegated_hashes.puzzle_hash.into() {
        return Ok(delegated_hashes.fixed_conditions_hash);
    }
    let member_hashes = p2_conditions_or_singleton_from_member_fixed(stored_hash, launcher_id)?;
    if binding_p2_puzzle_hash == member_hashes.puzzle_hash.into() {
        return Ok(member_hashes.fixed_conditions_hash);
    }
    Err(SignerError::Offer(
        OfferError::PresplitCoinPuzzleHashMismatch,
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SignerError;

    fn sample_binding() -> (Bytes32, TreeHash, TreeHash, Bytes32) {
        let launcher_id = Bytes32::new([0x11; 32]);
        let delegated = TreeHash::new([0x22; 32]);
        let hashes =
            p2_conditions_or_singleton_puzzle_hash(delegated, launcher_id).expect("p2 hashes");
        let binding_p2: Bytes32 = hashes.puzzle_hash.into();
        (
            launcher_id,
            delegated,
            hashes.fixed_conditions_hash,
            binding_p2,
        )
    }

    #[test]
    fn resolve_accepts_greenfloor_delegated_hash() {
        let (launcher_id, delegated, member_hash, binding_p2) = sample_binding();
        let resolved =
            resolve_member_fixed_conditions_hash_for_binding(launcher_id, binding_p2, delegated)
                .expect("resolve delegated");
        assert_eq!(resolved, member_hash);
    }

    #[test]
    fn resolve_accepts_cloud_wallet_member_wrapped_hash() {
        let (launcher_id, _delegated, member_hash, binding_p2) = sample_binding();
        let resolved =
            resolve_member_fixed_conditions_hash_for_binding(launcher_id, binding_p2, member_hash)
                .expect("resolve member-wrapped");
        assert_eq!(resolved, member_hash);
    }

    #[test]
    fn resolve_rejects_unrelated_hash() {
        let (launcher_id, _delegated, _member_hash, binding_p2) = sample_binding();
        let err = resolve_member_fixed_conditions_hash_for_binding(
            launcher_id,
            binding_p2,
            TreeHash::new([0x33; 32]),
        )
        .expect_err("unrelated hash");
        assert!(matches!(
            err,
            SignerError::Offer(OfferError::PresplitCoinPuzzleHashMismatch)
        ));
    }
}

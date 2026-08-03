//! Guards against double-wrapping vault CAT creates.
//!
//! `Action::send` for a CAT takes an **inner** p2 puzzle hash. The CAT layer remaps
//! `CREATE_COIN` to `cat(asset, p2)`. Passing the already-wrapped receive outer
//! (`cat(asset, receive_p2)`) as the send destination produces an unspendable
//! double-wrapped coin (`cat(asset, cat(asset, receive_p2))`).
//!
//! Call [`assert_cat_creates`] on `Spends` outputs after `prepare` — that is the
//! authoritative check. Prefer plain `Action::send(..., receive_p2, ...)` at
//! construction time; do not pass the CAT outer as the send destination.

use chia_protocol::Bytes32;
use chia_puzzle_types::cat::CatArgs;
use chia_sdk_driver::{Cat, Outputs};

use crate::error::{SignerError, SignerResult};

/// Outer puzzle hash for a single-wrapped CAT on `receive_p2`.
#[must_use]
pub(crate) fn receive_cat_outer_puzzle_hash(asset_id: Bytes32, receive_p2: Bytes32) -> Bytes32 {
    CatArgs::curry_tree_hash(asset_id, receive_p2.into()).into()
}

/// CAT creates recorded on a [`Outputs`] map (after `Spends::prepare`).
pub(crate) fn created_cats(outputs: &Outputs) -> impl Iterator<Item = &Cat> {
    outputs.cats.values().flat_map(|cats| cats.iter())
}

/// Assert every created CAT for `asset_id` lands on `receive_p2` (or an allowlisted
/// p2 such as settlement), never on `cat(asset, receive_p2)`.
///
/// # Errors
///
/// - [`SignerError::VaultCatCreateDestinationIsOuterLayer`] when a create used the
///   receive CAT outer as its p2 (the double-wrap regression).
/// - [`SignerError::VaultCatCreateDestinationNotReceiveP2`] when a create used any
///   other unexpected p2.
pub(crate) fn assert_cat_creates<'a>(
    cats: impl IntoIterator<Item = &'a Cat>,
    asset_id: Bytes32,
    receive_p2: Bytes32,
    allowed_non_receive_p2s: &[Bytes32],
) -> SignerResult<()> {
    let receive_outer = receive_cat_outer_puzzle_hash(asset_id, receive_p2);
    for cat in cats {
        if cat.info.asset_id != asset_id {
            continue;
        }
        let p2 = cat.info.p2_puzzle_hash;
        if p2 == receive_outer {
            return Err(SignerError::VaultCatCreateDestinationIsOuterLayer);
        }
        if p2 == receive_p2 || allowed_non_receive_p2s.contains(&p2) {
            continue;
        }
        return Err(SignerError::VaultCatCreateDestinationNotReceiveP2);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::Coin;
    use chia_sdk_driver::CatInfo;

    fn sample_cat(asset_id: Bytes32, p2: Bytes32, amount: u64) -> Cat {
        let info = CatInfo::new(asset_id, None, p2);
        Cat::new(
            Coin::new(Bytes32::new([0x22; 32]), info.puzzle_hash().into(), amount),
            None,
            info,
        )
    }

    #[test]
    fn accepts_receive_and_allowlisted_p2s() {
        let asset = Bytes32::new([0xaa; 32]);
        let receive = Bytes32::new([0x11; 32]);
        let settlement = Bytes32::new([0xef; 32]);
        let receive_cat = sample_cat(asset, receive, 1_000);
        let settlement_cat = sample_cat(asset, settlement, 500);
        assert_cat_creates(
            [&receive_cat, &settlement_cat],
            asset,
            receive,
            &[settlement],
        )
        .expect("ok");
    }

    #[test]
    fn rejects_outer_as_p2() {
        let asset = Bytes32::new([0xaa; 32]);
        let receive = Bytes32::new([0x11; 32]);
        let outer = receive_cat_outer_puzzle_hash(asset, receive);
        let cat = sample_cat(asset, outer, 1_000);
        let err = assert_cat_creates([&cat], asset, receive, &[]).unwrap_err();
        assert!(matches!(
            err,
            SignerError::VaultCatCreateDestinationIsOuterLayer
        ));
    }

    #[test]
    fn rejects_unexpected_p2() {
        let asset = Bytes32::new([0xaa; 32]);
        let receive = Bytes32::new([0x11; 32]);
        let other = Bytes32::new([0x33; 32]);
        let cat = sample_cat(asset, other, 1_000);
        let err = assert_cat_creates([&cat], asset, receive, &[]).unwrap_err();
        assert!(matches!(
            err,
            SignerError::VaultCatCreateDestinationNotReceiveP2
        ));
    }

    #[test]
    fn ignores_other_assets() {
        let asset = Bytes32::new([0xaa; 32]);
        let other_asset = Bytes32::new([0xbb; 32]);
        let receive = Bytes32::new([0x11; 32]);
        let cat = sample_cat(other_asset, Bytes32::new([0x33; 32]), 1_000);
        assert_cat_creates([&cat], asset, receive, &[]).expect("other asset ignored");
    }
}

use chia_protocol::{Bytes32, Coin, SpendBundle};
use chia_sdk_driver::Cat;
use clvm_utils::TreeHash;

use crate::error::{SignerError, SignerResult};
use crate::offer::presplit::cancel_binding::{self, PresplitBindingLookup, PresplitCoinBinding};

/// Presplit offer binding: fixed CONDITIONS hash and maker P2 puzzle hash.
///
/// `expires_at` is set from offer terms during planning. When recovered from a
/// cancellable maker coin spend, `expires_at` is `None` and cancel/reclaim uses
/// coin amount plus the parsed fixed-conditions hash only.
#[derive(Debug, Clone)]
pub struct PresplitOfferBinding {
    pub offer_amount: u64,
    pub expires_at: Option<u64>,
    pub fixed_conditions_tree_hash: TreeHash,
    pub p2_puzzle_hash: Bytes32,
}

impl PresplitOfferBinding {
    #[must_use]
    pub(crate) fn from_coin_binding(coin: Coin, binding: &PresplitCoinBinding) -> Self {
        Self {
            offer_amount: coin.amount,
            expires_at: None,
            fixed_conditions_tree_hash: binding.fixed_conditions_tree_hash,
            p2_puzzle_hash: binding.binding_p2_puzzle_hash,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn from_presplit_coin_input(
        launcher_id: Bytes32,
        coin: Coin,
        spend_bundle: &SpendBundle,
    ) -> SignerResult<Self> {
        let binding = match cancel_binding::presplit_binding_from_coin_input(
            launcher_id,
            coin,
            spend_bundle,
        )? {
            PresplitBindingLookup::Found(binding) => binding,
            // Callers reach this after selecting a cancellable presplit maker input.
            PresplitBindingLookup::NotPresplitMaker => {
                return Err(SignerError::OfferCancelNoSpendableInput);
            }
        };
        Ok(Self::from_coin_binding(coin, &binding))
    }
}

/// Verify presplit cat offer binding.
///
/// # Errors
///
/// Returns an error when the presplit CAT P2 puzzle hash does not match the binding.
pub fn verify_presplit_cat_offer_binding(
    presplit_cat: &Cat,
    binding: &PresplitOfferBinding,
) -> SignerResult<()> {
    if presplit_cat.info.p2_puzzle_hash != binding.p2_puzzle_hash {
        return Err(SignerError::PresplitCoinPuzzleHashMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::presplit::predict_presplit_cat;
    use crate::offer::types::OfferTerms;

    #[test]
    fn verify_presplit_binding_rejects_mismatched_p2_hash() {
        let launcher_id = Bytes32::new([0xcc; 32]);
        let source_cat = Cat::new(
            Coin::new(Bytes32::new([0x01; 32]), Bytes32::default(), 1000),
            None,
            chia_sdk_driver::CatInfo::new(Bytes32::new([0x02; 32]), None, Bytes32::default()),
        );
        let terms = OfferTerms {
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w"
                .to_string(),
            offer_asset_id: hex::encode(Bytes32::new([0x02; 32])),
            offer_amount: 1000,
            request_asset_id: "xch".to_string(),
            request_amount: 1,
            expires_at: None,
        };
        let binding =
            PresplitOfferBinding::plan(launcher_id, &terms, Bytes32::default(), Bytes32::default())
                .expect("binding");
        let mismatched_cat = predict_presplit_cat(&source_cat, Bytes32::new([0x99; 32]), 1000);
        let err = verify_presplit_cat_offer_binding(&mismatched_cat, &binding).unwrap_err();
        assert!(matches!(err, SignerError::PresplitCoinPuzzleHashMismatch));
    }
}

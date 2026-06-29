mod binding;
mod build;
mod cancel_binding;
mod conditions;
mod split;

use chia_protocol::Bytes32;
use chia_sdk_driver::{Cat, Offer};

pub use binding::{verify_presplit_cat_offer_binding, PresplitOfferBinding};
pub(crate) use build::build_offer_from_presplit_cat;
#[cfg(any(test, feature = "test-support"))]
#[allow(unused_imports)]
// re-exported for test_support; clippy lib pass does not resolve consumers
pub(crate) use build::{build_offer_from_presplit_input, PresplitMakerInput};
pub(crate) use cancel_binding::{
    offer_maker_cat_from_coin_input, presplit_binding_from_coin_input,
    verify_fixed_delegated_puzzle_hash_for_binding, PresplitBindingLookup,
};
pub use conditions::build_presplit_conditions_inner_spend;
pub(crate) use conditions::build_presplit_offer_cancel_inner_spend;
pub use split::{
    build_presplit_split_spend_bundle, predict_presplit_cat, validate_presplit_source_cats,
    vault_change_puzzle_hash,
};
#[cfg(any(test, feature = "test-support"))]
#[allow(unused_imports)]
// re-exported for test_support; clippy lib pass does not resolve consumers
pub(crate) use split::{build_presplit_split_spend_bundle_with_vault, PresplitSplitParams};

#[must_use]
pub fn offer_nonce_from_cats(cats: &[Cat]) -> Bytes32 {
    Offer::nonce(cats.iter().map(|cat| cat.coin.coin_id()).collect())
}

#[must_use]
pub fn offer_nonce_from_coin_ids(coin_ids: &[Bytes32]) -> Bytes32 {
    Offer::nonce(coin_ids.to_vec())
}

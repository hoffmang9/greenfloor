use chia_protocol::{Bytes32, SpendBundle};
use clvm_utils::TreeHash;

use crate::coinset::OfferCoinsetBackend;
use crate::error::{SignerError, SignerResult};
use chia_sdk_driver::{Cat, Vault};

/// Test-only backend that returns errors for every method.
pub(crate) struct EmptyOfferCoinset;

impl OfferCoinsetBackend for EmptyOfferCoinset {
    async fn select_cats_for_spend(
        &self,
        _receive_address: &str,
        _asset_id: Bytes32,
        _explicit_coin_ids: &[Bytes32],
        _target_amount: u64,
    ) -> SignerResult<crate::coinset::SelectedCats> {
        Err(SignerError::Other("unused".to_string()))
    }

    async fn fetch_latest_vault(
        &self,
        _launcher_id: Bytes32,
        _inner_puzzle_hash: TreeHash,
    ) -> SignerResult<Vault> {
        Err(SignerError::Other("unused".to_string()))
    }

    async fn fetch_unspent_offer_input_cat(
        &self,
        _coin_id: Bytes32,
        _inner_puzzle_hash: Option<Bytes32>,
        _amount: Option<u64>,
    ) -> SignerResult<Cat> {
        Err(SignerError::Other("unused".to_string()))
    }

    async fn wait_for_unspent_cat(&self, _coin_id: Bytes32) -> SignerResult<Cat> {
        Err(SignerError::Other("unused".to_string()))
    }

    async fn broadcast_spend_bundle(&self, _spend_bundle: SpendBundle) -> SignerResult<String> {
        Err(SignerError::Other("unused".to_string()))
    }
}

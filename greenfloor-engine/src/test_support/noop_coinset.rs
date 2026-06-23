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

    async fn fetch_offer_input_cat(&self, _coin_id: Bytes32) -> SignerResult<Cat> {
        Err(SignerError::Other("unused".to_string()))
    }

    async fn offer_input_coin_is_spent(&self, _coin_id: Bytes32) -> SignerResult<bool> {
        Err(SignerError::Other("unused".to_string()))
    }

    async fn wait_for_unspent_cat(&self, _coin_id: Bytes32) -> SignerResult<Cat> {
        Err(SignerError::Other("unused".to_string()))
    }

    async fn broadcast_spend_bundle(&self, _spend_bundle: SpendBundle) -> SignerResult<String> {
        Err(SignerError::Other("unused".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use chia_bls;
    use chia_protocol::{Bytes32, SpendBundle};

    use super::EmptyOfferCoinset;
    use crate::coinset::OfferCoinsetBackend;
    use crate::error::SignerError;

    #[tokio::test]
    async fn empty_offer_coinset_returns_unused_for_every_method() {
        let backend = EmptyOfferCoinset;
        let coin_id = Bytes32::new([0x01; 32]);
        let asset_id = Bytes32::new([0x02; 32]);
        let bundle = SpendBundle::new(vec![], chia_bls::Signature::default());
        let err = backend
            .select_cats_for_spend("xch1addr", asset_id, &[], 1)
            .await
            .unwrap_err();
        assert!(matches!(err, SignerError::Other(ref msg) if msg == "unused"));
        assert!(matches!(
            backend
                .fetch_latest_vault(coin_id, clvm_utils::TreeHash::new([0; 32]))
                .await
                .unwrap_err(),
            SignerError::Other(ref msg) if msg == "unused"
        ));
        assert!(matches!(
            backend.fetch_offer_input_cat(coin_id).await.unwrap_err(),
            SignerError::Other(ref msg) if msg == "unused"
        ));
        assert!(matches!(
            backend.offer_input_coin_is_spent(coin_id).await.unwrap_err(),
            SignerError::Other(ref msg) if msg == "unused"
        ));
        assert!(matches!(
            backend.wait_for_unspent_cat(coin_id).await.unwrap_err(),
            SignerError::Other(ref msg) if msg == "unused"
        ));
        assert!(matches!(
            backend.broadcast_spend_bundle(bundle).await.unwrap_err(),
            SignerError::Other(ref msg) if msg == "unused"
        ));
    }
}

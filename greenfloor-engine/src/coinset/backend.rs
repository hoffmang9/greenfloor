use chia_protocol::{Bytes32, SpendBundle};
use clvm_utils::TreeHash;

use super::{broadcast, coin_select, presplit, vault_fetch, CoinsetClient, SelectedCats};
use crate::error::SignerResult;
use chia_sdk_driver::{Cat, Vault};

pub struct LiveCoinset<'a>(pub &'a CoinsetClient);

pub trait OfferCoinsetBackend {
    fn select_cats_for_spend(
        &self,
        receive_address: &str,
        asset_id: Bytes32,
        explicit_coin_ids: &[Bytes32],
        target_amount: u64,
    ) -> impl std::future::Future<Output = SignerResult<SelectedCats>> + Send;

    fn fetch_latest_vault(
        &self,
        launcher_id: Bytes32,
        inner_puzzle_hash: TreeHash,
    ) -> impl std::future::Future<Output = SignerResult<Vault>> + Send;

    fn fetch_offer_input_cat(
        &self,
        coin_id: Bytes32,
    ) -> impl std::future::Future<Output = SignerResult<Cat>> + Send;

    fn offer_input_coin_is_spent(
        &self,
        coin_id: Bytes32,
    ) -> impl std::future::Future<Output = SignerResult<bool>> + Send;

    fn wait_for_unspent_cat(
        &self,
        coin_id: Bytes32,
    ) -> impl std::future::Future<Output = SignerResult<Cat>> + Send;

    fn broadcast_spend_bundle(
        &self,
        spend_bundle: SpendBundle,
    ) -> impl std::future::Future<Output = SignerResult<String>> + Send;
}

impl OfferCoinsetBackend for LiveCoinset<'_> {
    async fn select_cats_for_spend(
        &self,
        receive_address: &str,
        asset_id: Bytes32,
        explicit_coin_ids: &[Bytes32],
        target_amount: u64,
    ) -> SignerResult<SelectedCats> {
        coin_select::select_cats_for_spend(
            self.0,
            receive_address,
            asset_id,
            explicit_coin_ids,
            target_amount,
        )
        .await
    }

    async fn fetch_latest_vault(
        &self,
        launcher_id: Bytes32,
        inner_puzzle_hash: TreeHash,
    ) -> SignerResult<Vault> {
        vault_fetch::fetch_latest_vault(self.0, launcher_id, inner_puzzle_hash).await
    }

    async fn fetch_offer_input_cat(&self, coin_id: Bytes32) -> SignerResult<Cat> {
        presplit::fetch_offer_input_cat(self.0, coin_id).await
    }

    async fn offer_input_coin_is_spent(&self, coin_id: Bytes32) -> SignerResult<bool> {
        presplit::offer_input_coin_is_spent(self.0, coin_id).await
    }

    async fn wait_for_unspent_cat(&self, coin_id: Bytes32) -> SignerResult<Cat> {
        presplit::wait_for_unspent_cat(self.0, coin_id).await
    }

    async fn broadcast_spend_bundle(&self, spend_bundle: SpendBundle) -> SignerResult<String> {
        Ok(broadcast::broadcast_spend_bundle(self.0, spend_bundle)
            .await?
            .status)
    }
}

#[cfg(test)]
mod tests {
    use chia_bls;
    use chia_protocol::SpendBundle;
    use mockito::Server;

    use super::LiveCoinset;
    use crate::coinset::test_support::{cat_with_amount, mock_get_coin_record_by_name_body};
    use crate::coinset::{CoinsetClient, OfferCoinsetBackend};

    #[tokio::test]
    async fn live_coinset_reports_unspent_offer_input_coin() {
        let cat = cat_with_amount(1_000);
        let coin_id = cat.coin.coin_id();
        let body = mock_get_coin_record_by_name_body(&cat.coin, 0);
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_record_by_name")
            .with_status(200)
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        let client = CoinsetClient::new(server.url());
        let backend = LiveCoinset(&client);
        assert!(!backend
            .offer_input_coin_is_spent(coin_id)
            .await
            .expect("coin record lookup"));
    }

    #[tokio::test]
    async fn live_coinset_broadcast_delegates_push_tx_status() {
        let bundle = SpendBundle::new(vec![], chia_bls::Signature::default());
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("POST", "/push_tx")
            .with_status(200)
            .with_body(r#"{"success":true,"status":"SUCCESS"}"#)
            .expect(1)
            .create_async()
            .await;
        let client = CoinsetClient::new(server.url());
        let backend = LiveCoinset(&client);
        assert_eq!(
            backend
                .broadcast_spend_bundle(bundle)
                .await
                .expect("broadcast"),
            "SUCCESS"
        );
    }
}

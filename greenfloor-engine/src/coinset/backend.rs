use chia_protocol::SpendBundle;
use chia_protocol::{Bytes32, Coin};
use clvm_utils::TreeHash;

use super::{
    broadcast_spend_bundle, fetch_latest_vault, presplit, select_xch_for_amount, CoinsetClient,
    SelectedCats, XchSelectRequest,
};
use crate::error::{SignerError, SignerResult};
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

    fn fetch_presplit_cat_by_id(
        &self,
        coin_id: Bytes32,
    ) -> impl std::future::Future<Output = SignerResult<Cat>> + Send;

    fn wait_for_unspent_cat(
        &self,
        coin_id: Bytes32,
    ) -> impl std::future::Future<Output = SignerResult<Cat>> + Send;

    fn broadcast_spend_bundle(
        &self,
        spend_bundle: SpendBundle,
    ) -> impl std::future::Future<Output = SignerResult<String>> + Send;
}

impl<'a> LiveCoinset<'a> {
    /// Select XCH inputs for a target amount (explicit coin ids or wallet listing).
    pub(crate) async fn select_xch_for_spend(
        &self,
        receive_address: &str,
        explicit_coin_ids: &[Bytes32],
        amount: u64,
    ) -> SignerResult<Vec<Coin>> {
        select_xch_for_amount(XchSelectRequest {
            client: self.0,
            receive_address,
            explicit_coin_ids,
            amount,
            empty_err: SignerError::NoUnspentXchCoins,
            select_failed: SignerError::InsufficientXchFeeBalanceForMixedSplit,
        })
        .await
    }
}

impl OfferCoinsetBackend for LiveCoinset<'_> {
    async fn select_cats_for_spend(
        &self,
        receive_address: &str,
        asset_id: Bytes32,
        explicit_coin_ids: &[Bytes32],
        target_amount: u64,
    ) -> SignerResult<SelectedCats> {
        super::select_cats_for_spend(
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
        fetch_latest_vault(self.0, launcher_id, inner_puzzle_hash).await
    }

    async fn fetch_presplit_cat_by_id(&self, coin_id: Bytes32) -> SignerResult<Cat> {
        presplit::fetch_presplit_cat_by_id(self.0, coin_id).await
    }

    async fn wait_for_unspent_cat(&self, coin_id: Bytes32) -> SignerResult<Cat> {
        presplit::wait_for_unspent_cat(self.0, coin_id).await
    }

    async fn broadcast_spend_bundle(&self, spend_bundle: SpendBundle) -> SignerResult<String> {
        Ok(broadcast_spend_bundle(self.0, spend_bundle).await?.status)
    }
}

use std::collections::HashMap;
use std::sync::Mutex;

use chia_protocol::{Bytes32, SpendBundle};
use clvm_utils::TreeHash;

use super::harness::{
    fetch_cat_from_sim, fetch_cat_from_sim_by_id, fetch_vault_from_sim, SimChain,
};
use crate::coinset::coin_select::{finalize_selected_cats, SelectedCats};
use crate::coinset::{OfferCoinsetBackend, OfferInputCatLookup};
use crate::error::{SignerError, SignerResult};
use chia_sdk_driver::{Cat, Vault};

pub(crate) struct SimulatorOfferCoinset<'a> {
    chain: &'a SimChain,
    known_cats: Mutex<HashMap<Bytes32, Cat>>,
}

impl<'a> SimulatorOfferCoinset<'a> {
    pub fn new(chain: &'a SimChain) -> Self {
        Self {
            chain,
            known_cats: Mutex::new(HashMap::default()),
        }
    }

    pub fn register_cat(&self, cat: Cat) {
        self.known_cats
            .lock()
            .expect("known cats lock")
            .insert(cat.coin.coin_id(), cat);
    }

    fn list_unspent_cats(&self, asset_id: Bytes32) -> SignerResult<Vec<Cat>> {
        let sim = self.chain.sim.lock().expect("sim lock");
        let mut cats = Vec::new();
        for coin in sim.unspent_coins(self.chain.p2_message_hash, false) {
            let cat = fetch_cat_from_sim(&sim, coin).map_err(SignerError::Other)?;
            if cat.info.asset_id == asset_id {
                cats.push(cat);
            }
        }
        Ok(cats)
    }

    fn fetch_by_id(&self, coin_id: Bytes32) -> SignerResult<Cat> {
        if let Some(cat) = self
            .known_cats
            .lock()
            .expect("known cats lock")
            .get(&coin_id)
            .copied()
        {
            return Ok(cat);
        }
        fetch_cat_from_sim_by_id(self.chain, coin_id).map_err(SignerError::Other)
    }

    fn fetch_by_inner_puzzle(&self, inner_puzzle_hash: Bytes32, amount: u64) -> SignerResult<Cat> {
        let sim = self.chain.sim.lock().expect("sim lock");
        for cat in self
            .known_cats
            .lock()
            .expect("known cats lock")
            .values()
            .copied()
        {
            if cat.info.p2_puzzle_hash == inner_puzzle_hash
                && cat.coin.amount == amount
                && sim
                    .coin_state(cat.coin.coin_id())
                    .is_some_and(|state| state.spent_height.is_none())
            {
                return Ok(cat);
            }
        }
        for coin in sim.unspent_coins(self.chain.p2_message_hash, false) {
            let cat = fetch_cat_from_sim(&sim, coin).map_err(SignerError::Other)?;
            if cat.info.p2_puzzle_hash == inner_puzzle_hash && cat.coin.amount == amount {
                return Ok(cat);
            }
        }
        Err(SignerError::PresplitCoinNotFound)
    }
}

impl OfferCoinsetBackend for SimulatorOfferCoinset<'_> {
    async fn select_cats_for_spend(
        &self,
        _receive_address: &str,
        asset_id: Bytes32,
        explicit_coin_ids: &[Bytes32],
        target_amount: u64,
    ) -> SignerResult<SelectedCats> {
        let cats = if explicit_coin_ids.is_empty() {
            self.list_unspent_cats(asset_id)?
        } else {
            let mut cats = Vec::new();
            for coin_id in explicit_coin_ids {
                cats.push(self.fetch_by_id(*coin_id)?);
            }
            cats
        };
        finalize_selected_cats(cats, explicit_coin_ids, target_amount)
    }

    async fn fetch_latest_vault(
        &self,
        launcher_id: Bytes32,
        inner_puzzle_hash: TreeHash,
    ) -> SignerResult<Vault> {
        fetch_vault_from_sim(
            &self.chain.sim.lock().expect("sim lock"),
            launcher_id,
            inner_puzzle_hash,
        )
        .map_err(SignerError::Other)
    }

    async fn fetch_offer_input_cat(&self, lookup: OfferInputCatLookup) -> SignerResult<Cat> {
        match lookup {
            OfferInputCatLookup::ByCoinId(coin_id) => match self.fetch_by_id(coin_id) {
                Ok(cat) => Ok(cat),
                Err(SignerError::PresplitCoinNotFound | SignerError::Other(_)) => {
                    Err(SignerError::PresplitCoinNotFound)
                }
                Err(err) => Err(err),
            },
            OfferInputCatLookup::ByCatFingerprint {
                inner_puzzle_hash,
                amount,
                ..
            } => self.fetch_by_inner_puzzle(inner_puzzle_hash, amount),
        }
    }

    async fn wait_for_unspent_cat(&self, coin_id: Bytes32) -> SignerResult<Cat> {
        self.fetch_by_id(coin_id)
    }

    async fn broadcast_spend_bundle(&self, spend_bundle: SpendBundle) -> SignerResult<String> {
        self.chain
            .sim
            .lock()
            .expect("sim lock")
            .spend_coins(spend_bundle.coin_spends, &[])
            .map_err(|err| SignerError::Other(err.to_string()))?;
        Ok("SUCCESS".to_string())
    }
}

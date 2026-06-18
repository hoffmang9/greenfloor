use std::collections::HashSet;

use chia_protocol::Bytes32;

use crate::coin_ops::SpendableCoin;
use crate::coinset::{list_wallet_unspent_coins, spend_bundle_hash_from_hex};
use crate::config::{ManagerProgramConfig, MarketConfig, SignerConfig};
use crate::error::SignerResult;
use crate::vault::{
    build_and_optionally_broadcast_vault_cat_mixed_split, members::hex_to_bytes32,
    MixedSplitRequest,
};

use super::helpers::wallet_coins_to_spendable;

pub struct CoinOpExecContext {
    pub signer_config: SignerConfig,
    pub market: MarketConfig,
    pub program: ManagerProgramConfig,
    pub resolved_base_asset_id: String,
    pub base_unit_mojo_multiplier: i64,
    pub combine_input_cap: i64,
    pub watched_coin_ids: HashSet<String>,
}

impl CoinOpExecContext {
    pub async fn list_spendable_coins(&self) -> SignerResult<Vec<SpendableCoin>> {
        let coins = list_wallet_unspent_coins(
            &self.program.network,
            &self.market.receive_address,
            &self.resolved_base_asset_id,
        )
        .await?;
        Ok(wallet_coins_to_spendable(
            &coins,
            self.market.base_asset.trim(),
        ))
    }

    pub async fn execute_mixed_split(
        &self,
        output_amounts: Vec<u64>,
        coin_ids: &[String],
        fee_mojos: u64,
    ) -> SignerResult<String> {
        let asset_id = hex_to_bytes32(&self.resolved_base_asset_id)?;
        let parsed_coin_ids: Vec<Bytes32> = coin_ids
            .iter()
            .map(|coin_id| hex_to_bytes32(coin_id))
            .collect::<SignerResult<Vec<_>>>()?;
        let request = MixedSplitRequest {
            receive_address: self.market.receive_address.clone(),
            asset_id,
            output_amounts,
            coin_ids: parsed_coin_ids,
            allow_sub_cat_output: false,
            fee_mojos,
        };
        let result = build_and_optionally_broadcast_vault_cat_mixed_split(
            self.signer_config.clone(),
            request,
            true,
        )
        .await?;
        spend_bundle_hash_from_hex(&result.spend_bundle_hex)
    }
}

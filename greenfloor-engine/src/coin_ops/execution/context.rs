use std::collections::HashSet;

use chia_protocol::Bytes32;

use crate::coin_ops::{
    coin_op_non_negative_u64, combine_output_amounts, total_for_coin_ids, SpendableCoin,
    COMBINE_SINGLE_OUTPUT_COUNT,
};
use crate::coinset::{list_wallet_unspent_coins_for_signer, spend_bundle_hash_from_hex};
use crate::config::{ManagerProgramConfig, MarketConfig, SignerConfig};
use crate::error::SignerResult;
use crate::hex::{default_mojo_multiplier_for_asset, hex_to_bytes32};
use crate::offer::resolve_market_base_asset_id;
use crate::vault::{build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest};

use super::cap::resolve_combine_input_cap;
use super::helpers::wallet_coins_to_spendable;
#[cfg(test)]
use super::test_overrides::CoinOpTestOverrides;

pub struct CoinOpExecContext {
    pub signer_config: SignerConfig,
    pub market: MarketConfig,
    pub program: ManagerProgramConfig,
    pub resolved_base_asset_id: String,
    pub base_unit_mojo_multiplier: i64,
    pub combine_input_cap: i64,
    pub watched_coin_ids: HashSet<String>,
    #[cfg(test)]
    pub test_overrides: CoinOpTestOverrides,
}

impl CoinOpExecContext {
    /// Build execution context from loaded operator config.
    ///
    /// # Errors
    ///
    /// Returns an error if asset resolution fails.
    pub async fn new(
        program: ManagerProgramConfig,
        signer_config: SignerConfig,
        market: MarketConfig,
        canonical_base_asset: Option<&str>,
        watched_coin_ids: HashSet<String>,
        #[cfg(test)] test_overrides: CoinOpTestOverrides,
    ) -> SignerResult<Self> {
        let canonical = canonical_base_asset
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| market.base_asset.trim());
        let resolved_base_asset_id =
            resolve_market_base_asset_id(&signer_config, canonical).await?;
        Ok(Self {
            signer_config,
            market: market.clone(),
            program,
            resolved_base_asset_id,
            base_unit_mojo_multiplier: default_mojo_multiplier_for_asset(market.base_asset.trim()),
            combine_input_cap: resolve_combine_input_cap(),
            watched_coin_ids,
            #[cfg(test)]
            test_overrides,
        })
    }

    /// Submit a combine: merge `input_coin_ids` into a single output coin.
    ///
    /// When `spendable` is `None`, wallet coins are fetched from Coinset.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn execute_combine(
        &self,
        input_coin_ids: &[String],
        spendable: Option<&[SpendableCoin]>,
    ) -> SignerResult<String> {
        let total = if let Some(coins) = spendable {
            total_for_coin_ids(coins, input_coin_ids)
        } else {
            let fetched = self.list_spendable_coins().await?;
            total_for_coin_ids(&fetched, input_coin_ids)
        };
        let output_amounts = combine_output_amounts(total, COMBINE_SINGLE_OUTPUT_COUNT)?;
        let fee_mojos = coin_op_non_negative_u64(
            self.program.coin_ops_combine_fee_mojos,
            "program.coin_ops_combine_fee_mojos",
        )?;
        self.execute_mixed_split(output_amounts, input_coin_ids, fee_mojos)
            .await
    }

    /// List spendable coins.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn list_spendable_coins(&self) -> SignerResult<Vec<SpendableCoin>> {
        #[cfg(test)]
        if let Some(coins) = self.test_overrides.wallet_coins_override() {
            return Ok(coins.to_vec());
        }
        let coins = list_wallet_unspent_coins_for_signer(
            &self.program.network,
            &self.signer_config,
            &self.market.receive_address,
            &self.resolved_base_asset_id,
        )
        .await?;
        Ok(wallet_coins_to_spendable(
            &coins,
            self.market.base_asset.trim(),
        ))
    }

    /// Execute mixed split.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn execute_mixed_split(
        &self,
        output_amounts: Vec<u64>,
        coin_ids: &[String],
        fee_mojos: u64,
    ) -> SignerResult<String> {
        #[cfg(test)]
        if let Some(operation_id) = self.test_overrides.mixed_split_operation_id_override() {
            let _ = (output_amounts, coin_ids, fee_mojos);
            return Ok(operation_id.to_string());
        }
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
            None,
        )
        .await?;
        spend_bundle_hash_from_hex(&result.spend_bundle_hex)
    }
}

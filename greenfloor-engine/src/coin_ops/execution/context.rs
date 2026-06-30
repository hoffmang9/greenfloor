use std::collections::HashSet;

use chia_protocol::Bytes32;

use crate::coin_ops::{
    coin_op_non_negative_u64, combine_output_amounts, total_for_coin_ids, SpendableCoin,
    COMBINE_SINGLE_OUTPUT_COUNT,
};
use crate::coinset::{list_wallet_unspent_coins_for_signer, spend_bundle_hash_from_hex};
use crate::config::{GatedOperatorMarket, MarketConfig};
use crate::error::{SignerError, SignerResult};
use crate::hex::{default_mojo_multiplier_for_asset, hex_to_bytes32};
use crate::offer::OfferAssetResolver;
use crate::vault::{build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest};

use super::cap::resolve_combine_input_cap;
use super::helpers::wallet_coins_to_spendable;
#[cfg(test)]
use super::test_overrides::CoinOpTestOverrides;

pub struct CoinOpExecContext {
    pub gated: GatedOperatorMarket,
    pub resolved_base_asset_id: String,
    pub base_unit_mojo_multiplier: i64,
    pub combine_input_cap: i64,
    pub watched_coin_ids: HashSet<String>,
    #[cfg(test)]
    pub test_overrides: CoinOpTestOverrides,
}

impl CoinOpExecContext {
    /// Build execution context from an owned gated operator market.
    ///
    /// # Errors
    ///
    /// Returns an error if asset resolution fails.
    pub async fn from_gated_market(
        gated: GatedOperatorMarket,
        canonical_base_asset: Option<&str>,
        watched_coin_ids: HashSet<String>,
        #[cfg(test)] test_overrides: CoinOpTestOverrides,
    ) -> SignerResult<Self> {
        let resolver = gated.asset_resolver();
        let resolved_base_asset_id =
            resolve_base_asset_id(&resolver, &gated.market_row, canonical_base_asset).await?;
        Ok(Self::assemble(
            gated,
            resolved_base_asset_id,
            watched_coin_ids,
            #[cfg(test)]
            test_overrides,
        ))
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
            self.gated.program.coin_ops_combine_fee_mojos,
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
            &self.gated.operator_network,
            &self.gated.signer,
            &self.gated.market_row.receive_address,
            &self.resolved_base_asset_id,
        )
        .await?;
        Ok(wallet_coins_to_spendable(
            &coins,
            self.gated.market_row.base_asset.trim(),
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
        if self.test_overrides.take_mixed_split_stale_first_failure() {
            let _ = (output_amounts, coin_ids, fee_mojos);
            return Err(SignerError::MixedSplitSelectedCoinsNotSpendable);
        }
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
            receive_address: self.gated.market_row.receive_address.clone(),
            asset_id,
            output_amounts,
            coin_ids: parsed_coin_ids,
            allow_sub_cat_output: false,
            fee_mojos,
        };
        let result = build_and_optionally_broadcast_vault_cat_mixed_split(
            self.gated.signer.clone(),
            &self.gated.operator_network,
            request,
            true,
        )
        .await
        .map_err(SignerError::normalize_mixed_split_error)?;
        spend_bundle_hash_from_hex(&result.spend_bundle_hex)
    }

    fn assemble(
        gated: GatedOperatorMarket,
        resolved_base_asset_id: String,
        watched_coin_ids: HashSet<String>,
        #[cfg(test)] test_overrides: CoinOpTestOverrides,
    ) -> Self {
        Self {
            base_unit_mojo_multiplier: default_mojo_multiplier_for_asset(
                gated.market_row.base_asset.trim(),
            ),
            combine_input_cap: resolve_combine_input_cap(),
            gated,
            resolved_base_asset_id,
            watched_coin_ids,
            #[cfg(test)]
            test_overrides,
        }
    }
}

async fn resolve_base_asset_id(
    resolver: &OfferAssetResolver<'_>,
    market_row: &MarketConfig,
    canonical_base_asset: Option<&str>,
) -> SignerResult<String> {
    let canonical = canonical_base_asset
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| market_row.base_asset.trim());
    resolver.resolve_base(canonical).await
}

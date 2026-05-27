pub mod bls;
pub mod coinset;
pub mod config;
pub mod error;
pub mod kms;
pub mod offer;
pub mod vault;

use config::SignerConfig;
use error::SignerResult;

pub async fn resolve_vault_context(config: SignerConfig) -> SignerResult<vault::VaultContext> {
    Ok(vault::session::resolve_vault_session(config).await?.display)
}

pub async fn resolve_offer_asset_ids(
    config: SignerConfig,
    base_asset: &str,
    quote_asset: &str,
) -> SignerResult<(String, String)> {
    let msp = coinset::MspCoinset::for_network(&config.network, Some(&config.coinset_msp_base_url))?;
    coinset::resolve_offer_asset_ids(&msp, base_asset, quote_asset).await
}

pub use coinset::parse_coin_ids;
pub use config::{load_cloud_wallet_config, load_signer_config};
pub use error::SignerError as Error;
pub use offer::{build_vault_cat_offer, CreateOfferRequest, CreateOfferResult};
pub use bls::{
    broadcast_bls_spend_bundle, build_bls_mixed_split_spend_bundle, build_bls_offer_spend_bundle,
    build_bls_xch_coin_op_spend_bundle, list_cat_coin_summaries, list_cat_coin_summaries_by_ids,
    list_xch_coin_summaries, BlsMixedSplitRequest, BlsMixedSplitResult, BlsOfferRequest,
    BlsOfferResult, BlsXchCoinOpRequest, BlsXchCoinOpResult, CoinRecordSummary,
};
pub use vault::{
    build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest, MixedSplitResult,
};

#[cfg(test)]
mod test_support;

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
pub use vault::{
    build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest, MixedSplitResult,
};

#[cfg(test)]
mod test_support;

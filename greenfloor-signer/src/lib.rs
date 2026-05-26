pub mod cloud_wallet;
pub mod coinset;
pub mod config;
pub mod error;
pub mod kms;
pub mod offer;
pub mod vault;

use config::CloudWalletConfig;
use error::SignerResult;

pub async fn resolve_vault_context(config: CloudWalletConfig) -> SignerResult<vault::VaultContext> {
    Ok(vault::session::resolve_vault_session(config).await?.display)
}

pub use coinset::parse_coin_ids;
pub use config::load_cloud_wallet_config;
pub use error::SignerError as Error;
pub use offer::{build_vault_cat_offer, CreateOfferRequest, CreateOfferResult};
pub use vault::{
    build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest, MixedSplitResult,
};

#[cfg(test)]
mod test_support;

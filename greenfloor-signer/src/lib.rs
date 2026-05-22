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
    let kms_public_key_hex = match config.kms_public_key_hex.clone() {
        Some(value) => value,
        None => kms::get_public_key_compressed_hex(&config.kms_key_id, &config.kms_region).await?,
    };

    let client = cloud_wallet::CloudWalletClient::new(config.clone())?;
    let snapshot = client.get_vault_custody_snapshot().await?;
    vault::compute_vault_context(&snapshot, &kms_public_key_hex, &config.network)
}

pub use coinset::parse_coin_ids;
pub use config::load_cloud_wallet_config;
pub use error::SignerError as Error;
pub use offer::{CreateOfferRequest, CreateOfferResult, build_vault_cat_offer};
pub use vault::{
    MixedSplitRequest, MixedSplitResult, build_and_optionally_broadcast_vault_cat_mixed_split,
};

#[cfg(test)]
mod test_support;

use crate::config::SignerConfig;
use crate::error::SignerResult;
use crate::vault::context::{
    compute_vault_context_from_hashes, compute_vault_hashes, VaultContext, VaultCustodySnapshot,
};
use crate::vault::spend::{build_vault_spend_context_from_hashes, VaultSpendContext};

#[derive(Debug, Clone)]
pub struct VaultSession {
    pub display: VaultContext,
    pub spend: VaultSpendContext,
}

/// Resolve vault session.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn resolve_vault_session(config: SignerConfig) -> SignerResult<VaultSession> {
    let kms_public_key_hex = match config.kms_public_key_hex.clone() {
        Some(value) => value,
        None => {
            crate::kms::get_public_key_compressed_hex(
                &config.kms_runtime,
                &config.kms_key_id,
                &config.kms_region,
            )
            .await?
        }
    };

    build_vault_session(&config.vault, &kms_public_key_hex, &config)
}

/// Build vault session.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn build_vault_session(
    snapshot: &VaultCustodySnapshot,
    kms_public_key_hex: &str,
    config: &SignerConfig,
) -> SignerResult<VaultSession> {
    let hashes = compute_vault_hashes(snapshot)?;
    let display =
        compute_vault_context_from_hashes(snapshot, &hashes, kms_public_key_hex, &config.network)?;
    let spend = build_vault_spend_context_from_hashes(snapshot, &hashes, &display, config)?;
    Ok(VaultSession { display, spend })
}

/// Resolve vault spend context.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn resolve_vault_spend_context(config: SignerConfig) -> SignerResult<VaultSpendContext> {
    Ok(resolve_vault_session(config).await?.spend)
}

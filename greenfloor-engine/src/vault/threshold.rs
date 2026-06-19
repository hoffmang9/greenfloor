//! Vault custody threshold validation (YAML signer config and GraphQL snapshots).

use crate::error::{SignerError, SignerResult};

/// Validate vault threshold.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn validate_vault_threshold(threshold: u32, key_count: usize) -> SignerResult<()> {
    let threshold_usize =
        usize::try_from(threshold).map_err(|_| SignerError::UnsupportedVaultThreshold)?;
    if threshold == 0 || threshold_usize > key_count {
        return Err(SignerError::UnsupportedVaultThreshold);
    }
    Ok(())
}

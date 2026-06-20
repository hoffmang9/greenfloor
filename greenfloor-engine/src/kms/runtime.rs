//! Injectable KMS boundaries for production and unit tests.

use aws_sdk_kms::Client;

use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone, Default)]
pub struct KmsOverrides {
    pub public_key_compressed_hex: Option<String>,
    pub fast_fail: bool,
}

#[derive(Debug, Clone)]
pub struct KmsRuntime {
    overrides: KmsOverrides,
}

impl KmsRuntime {
    #[must_use]
    pub fn production() -> Self {
        Self {
            overrides: KmsOverrides::default(),
        }
    }

    #[must_use]
    pub fn test(overrides: KmsOverrides) -> Self {
        Self { overrides }
    }

    pub fn public_key_override(&self) -> Option<String> {
        self.overrides
            .public_key_compressed_hex
            .as_deref()
            .map(str::trim)
            .filter(|hex| !hex.is_empty())
            .map(str::to_string)
    }

    /// Reject KMS client creation when test fast-fail is configured.
    ///
    /// # Errors
    ///
    /// Returns an error when `fast_fail` is set on the active overrides.
    pub fn ensure_client_allowed(&self) -> SignerResult<()> {
        if self.overrides.fast_fail {
            return Err(SignerError::Kms(
                "credentials not configured (test fast fail)".to_string(),
            ));
        }
        Ok(())
    }

    /// Build an AWS KMS client for `region`.
    ///
    /// # Errors
    ///
    /// Returns an error when test fast-fail is configured or AWS config load fails.
    pub async fn client(&self, region: &str) -> SignerResult<Client> {
        self.ensure_client_allowed()?;
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .load()
            .await;
        Ok(Client::new(&config))
    }
}

impl Default for KmsRuntime {
    fn default() -> Self {
        Self::production()
    }
}

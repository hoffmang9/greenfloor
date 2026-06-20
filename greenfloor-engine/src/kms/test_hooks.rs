//! Injectable KMS test overrides (RAII guard, no environment variables).

use std::sync::{Mutex, MutexGuard};

use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone, Default)]
pub struct KmsTestOverrides {
    pub public_key_compressed_hex: Option<String>,
    pub fast_fail: bool,
}

struct ActiveKmsTestOverrides(Mutex<Option<KmsTestOverrides>>);

impl ActiveKmsTestOverrides {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    fn lock(&self) -> MutexGuard<'_, Option<KmsTestOverrides>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

static ACTIVE: ActiveKmsTestOverrides = ActiveKmsTestOverrides::new();
static KMS_TEST_SERIAL: Mutex<()> = Mutex::new(());

/// Serializes tests that mutate KMS overrides (cargo runs tests in parallel).
pub struct KmsTestGuard {
    _serial: MutexGuard<'static, ()>,
}

impl KmsTestGuard {
    #[must_use]
    pub fn new(overrides: KmsTestOverrides) -> Self {
        let serial = KMS_TEST_SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *ACTIVE.lock() = Some(overrides);
        Self { _serial: serial }
    }
}

impl Drop for KmsTestGuard {
    fn drop(&mut self) {
        *ACTIVE.lock() = None;
    }
}

pub(crate) fn public_key_stub() -> Option<String> {
    ACTIVE
        .lock()
        .as_ref()
        .and_then(|overrides| overrides.public_key_compressed_hex.clone())
        .map(|hex| hex.trim().to_string())
        .filter(|hex| !hex.is_empty())
}

pub(crate) fn kms_client_fast_fail() -> SignerResult<()> {
    if ACTIVE
        .lock()
        .as_ref()
        .is_some_and(|overrides| overrides.fast_fail)
    {
        return Err(SignerError::Kms(
            "credentials not configured (test fast fail)".to_string(),
        ));
    }
    Ok(())
}

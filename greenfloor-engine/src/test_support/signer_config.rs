//! Minimal signer config for unit tests (no KMS or coinset IO).

use chia_protocol::Bytes32;

use crate::config::SignerConfig;
use crate::vault::context::VaultCustodySnapshot;

#[must_use]
pub fn test_signer_config(msp_base_url: &str) -> SignerConfig {
    SignerConfig {
        network: "mainnet".to_string(),
        coinset_msp_base_url: msp_base_url.to_string(),
        kms_key_id: "kms-test".to_string(),
        kms_region: "us-west-2".to_string(),
        kms_public_key_hex: None,
        kms_runtime: crate::kms::KmsRuntime::default(),
        vault: VaultCustodySnapshot {
            launcher_id: Bytes32::default(),
            custody_threshold: 1,
            recovery_threshold: 1,
            recovery_clawback_timelock: 3600,
            custody_keys: Vec::new(),
            recovery_keys: Vec::new(),
        },
    }
}

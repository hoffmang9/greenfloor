use std::path::Path;

use serde_json::Value;

use super::program::read_program_yaml;
use super::yaml_fields::{config_err, optional_trimmed_string, req_mapping, req_str, req_value};
use crate::error::{SignerError, SignerResult};
use crate::hex::hex_to_bytes32;
use crate::kms::KmsRuntime;
use crate::vault::context::VaultCustodySnapshot;
use crate::vault::members::WalletKey;
use crate::vault::validate_vault_threshold;

use crate::coinset::DEFAULT_COINSET_BASE_URL;

#[derive(Debug, Clone)]
pub struct SignerConfig {
    pub network: String,
    pub coinset_base_url: String,
    pub kms_key_id: String,
    pub kms_region: String,
    pub kms_public_key_hex: Option<String>,
    pub kms_runtime: KmsRuntime,
    pub vault: VaultCustodySnapshot,
}

/// Parse signer config.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn parse_signer_config(raw: &Value) -> SignerResult<SignerConfig> {
    let network = raw
        .get("app")
        .and_then(Value::as_object)
        .and_then(|app| app.get("network"))
        .and_then(Value::as_str)
        .unwrap_or("mainnet")
        .trim()
        .to_string();

    let signer = req_mapping(raw, "signer")?;
    let vault = req_mapping(raw, "vault")?;

    let kms_key_id = require_nonempty_str(signer, "kms_key_id")?;
    let kms_region = signer
        .get("kms_region")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("us-west-2")
        .to_string();
    let kms_public_key_hex = optional_trimmed_string(signer.get("kms_public_key_hex"));
    let coinset_base_url = parse_signer_coinset_base_url(signer);

    let vault_snapshot = parse_vault_section(vault)?;

    Ok(SignerConfig {
        network,
        coinset_base_url,
        kms_key_id,
        kms_region,
        kms_public_key_hex,
        kms_runtime: KmsRuntime::production(),
        vault: vault_snapshot,
    })
}

fn parse_signer_coinset_base_url(signer: &serde_json::Map<String, Value>) -> String {
    let read = |key: &str| {
        signer
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    read("coinset_base_url")
        .unwrap_or(DEFAULT_COINSET_BASE_URL)
        .to_string()
}

/// Load signer config.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_signer_config(path: &Path) -> SignerResult<SignerConfig> {
    parse_signer_config(&read_program_yaml(path)?)
}

fn parse_vault_section(
    vault: &serde_json::Map<String, Value>,
) -> SignerResult<VaultCustodySnapshot> {
    let launcher_id = hex_to_bytes32(&require_nonempty_str(vault, "launcher_id")?)
        .map_err(|_| SignerError::VaultLauncherIdInvalid)?;
    let custody_threshold = parse_u32_field(
        req_value(vault, "custody_threshold")?,
        "vault.custody_threshold",
    )?;
    let recovery_threshold = parse_u32_field(
        req_value(vault, "recovery_threshold")?,
        "vault.recovery_threshold",
    )?;
    let recovery_clawback_timelock = parse_u64_field(
        req_value(vault, "recovery_clawback_timelock")?,
        "vault.recovery_clawback_timelock",
    )?;

    let custody_keys = parse_wallet_keys(req_value(vault, "custody_keys")?, "vault.custody_keys")?;
    let recovery_keys =
        parse_wallet_keys(req_value(vault, "recovery_keys")?, "vault.recovery_keys")?;

    if custody_keys.is_empty() || recovery_keys.is_empty() {
        return Err(SignerError::UnsupportedVaultSignerCardinality);
    }
    validate_vault_threshold(custody_threshold, custody_keys.len())?;
    validate_vault_threshold(recovery_threshold, recovery_keys.len())?;
    if recovery_clawback_timelock == 0 {
        return Err(SignerError::InvalidVaultRecoveryTimelock);
    }

    Ok(VaultCustodySnapshot {
        launcher_id,
        custody_threshold,
        recovery_threshold,
        recovery_clawback_timelock,
        custody_keys,
        recovery_keys,
    })
}

fn parse_wallet_keys(raw: &Value, field: &str) -> SignerResult<Vec<WalletKey>> {
    let entries = raw
        .as_array()
        .ok_or_else(|| config_err(format!("{field} must be a list")))?;
    entries
        .iter()
        .map(|entry| {
            let map = entry
                .as_object()
                .ok_or_else(|| config_err(format!("{field} entries must be mappings")))?;
            Ok(WalletKey {
                public_key_hex: req_str(map, "public_key_hex")?.trim().to_string(),
                curve: req_str(map, "curve")?.trim().to_string(),
            })
        })
        .collect()
}

fn require_nonempty_str(
    map: &serde_json::Map<String, Value>,
    key: &'static str,
) -> SignerResult<String> {
    let trimmed = req_str(map, key)?.trim().to_string();
    if trimmed.is_empty() {
        return Err(SignerError::MissingConfigField(key));
    }
    Ok(trimmed)
}

fn parse_u32_field(raw: &Value, context: &str) -> SignerResult<u32> {
    let value = super::yaml_fields::parse_i64_field(raw, context)?;
    if value < 0 {
        return Err(config_err(format!("{context} must be >= 0")));
    }
    u32::try_from(value).map_err(|_| config_err(format!("{context} must fit in u32")))
}

fn parse_u64_field(raw: &Value, context: &str) -> SignerResult<u64> {
    let value = super::yaml_fields::parse_i64_field(raw, context)?;
    if value < 0 {
        return Err(config_err(format!("{context} must be >= 0")));
    }
    crate::config::parse_non_negative_u64(value, context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_signer_and_vault_from_yaml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("program.yaml");
        let mut file = std::fs::File::create(&path).expect("create");
        write!(
            file,
            r#"
app:
  network: testnet11
signer:
  kms_key_id: arn:aws:kms:us-west-2:123:key/abc
  kms_region: us-west-2
  coinset_base_url: https://api.coinset.org
vault:
  launcher_id: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  custody_threshold: 1
  recovery_threshold: 1
  recovery_clawback_timelock: 3600
  custody_keys:
    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"
      curve: SECP256R1
  recovery_keys:
    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"
      curve: BLS12_381
"#
        )
        .expect("write");
        let cfg = load_signer_config(&path).expect("load");
        assert_eq!(cfg.network, "testnet11");
        assert_eq!(cfg.kms_key_id, "arn:aws:kms:us-west-2:123:key/abc");
        assert_eq!(cfg.vault.custody_threshold, 1);
        assert_eq!(hex::encode(cfg.vault.launcher_id), "aa".repeat(32));
    }
}

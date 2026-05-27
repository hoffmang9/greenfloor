use std::path::Path;

use serde::Deserialize;

use crate::error::{SignerError, SignerResult};
use crate::vault::context::VaultCustodySnapshot;
use crate::vault::members::{hex_to_bytes32, WalletKey};

pub use crate::coinset::DEFAULT_MSP_BASE_URL;

#[derive(Debug, Clone)]
pub struct SignerConfig {
    pub network: String,
    pub coinset_msp_base_url: String,
    pub kms_key_id: String,
    pub kms_region: String,
    pub kms_public_key_hex: Option<String>,
    pub vault: VaultCustodySnapshot,
}

#[derive(Debug, Deserialize)]
struct ProgramYaml {
    app: Option<AppSection>,
    signer: Option<SignerSection>,
    vault: Option<VaultSection>,
}

#[derive(Debug, Deserialize)]
struct AppSection {
    network: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SignerSection {
    coinset_msp_base_url: Option<String>,
    kms_key_id: Option<String>,
    kms_region: Option<String>,
    kms_public_key_hex: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VaultSection {
    launcher_id: Option<String>,
    custody_threshold: Option<u32>,
    recovery_threshold: Option<u32>,
    recovery_clawback_timelock: Option<u64>,
    custody_keys: Option<Vec<WalletKeyYaml>>,
    recovery_keys: Option<Vec<WalletKeyYaml>>,
}

#[derive(Debug, Deserialize)]
struct WalletKeyYaml {
    public_key_hex: String,
    curve: String,
}

pub fn load_signer_config(path: &Path) -> SignerResult<SignerConfig> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!("failed to read config {}: {err}", path.display()))
    })?;
    let parsed: ProgramYaml = serde_yaml::from_str(&raw).map_err(|err| {
        SignerError::Other(format!("failed to parse config {}: {err}", path.display()))
    })?;

    let network = parsed
        .app
        .and_then(|app| app.network)
        .unwrap_or_else(|| "mainnet".to_string());

    let signer = parsed
        .signer
        .ok_or(SignerError::MissingConfigField("signer"))?;
    let vault = parsed
        .vault
        .ok_or(SignerError::MissingConfigField("vault"))?;

    let kms_key_id = require_field(signer.kms_key_id, "signer.kms_key_id")?;
    let kms_region = signer
        .kms_region
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "us-west-2".to_string());
    let kms_public_key_hex = signer
        .kms_public_key_hex
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let coinset_msp_base_url = signer
        .coinset_msp_base_url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_MSP_BASE_URL.to_string());

    let vault_snapshot = vault_section_to_snapshot(vault)?;

    Ok(SignerConfig {
        network: network.trim().to_string(),
        coinset_msp_base_url,
        kms_key_id,
        kms_region,
        kms_public_key_hex,
        vault: vault_snapshot,
    })
}

fn vault_section_to_snapshot(vault: VaultSection) -> SignerResult<VaultCustodySnapshot> {
    let launcher_id = hex_to_bytes32(&require_field(vault.launcher_id, "vault.launcher_id")?)
        .map_err(|_| SignerError::VaultLauncherIdInvalid)?;
    let custody_threshold = vault
        .custody_threshold
        .ok_or(SignerError::VaultThresholdOrTimelockInvalid)?;
    let recovery_threshold = vault
        .recovery_threshold
        .ok_or(SignerError::VaultThresholdOrTimelockInvalid)?;
    let recovery_clawback_timelock = vault
        .recovery_clawback_timelock
        .ok_or(SignerError::VaultThresholdOrTimelockInvalid)?;

    let custody_keys = wallet_keys_from_yaml(
        vault.custody_keys.ok_or(SignerError::UnsupportedVaultSignerCardinality)?,
    )?;
    let recovery_keys = wallet_keys_from_yaml(
        vault.recovery_keys.ok_or(SignerError::UnsupportedVaultSignerCardinality)?,
    )?;

    if custody_keys.is_empty() || recovery_keys.is_empty() {
        return Err(SignerError::UnsupportedVaultSignerCardinality);
    }
    if custody_threshold == 0 || custody_threshold as usize > custody_keys.len() {
        return Err(SignerError::UnsupportedVaultThreshold);
    }
    if recovery_threshold == 0 || recovery_threshold as usize > recovery_keys.len() {
        return Err(SignerError::UnsupportedVaultThreshold);
    }
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

fn wallet_keys_from_yaml(entries: Vec<WalletKeyYaml>) -> SignerResult<Vec<WalletKey>> {
    entries
        .into_iter()
        .map(|entry| {
            Ok(WalletKey {
                public_key_hex: entry.public_key_hex.trim().to_string(),
                curve: entry.curve.trim().to_string(),
            })
        })
        .collect()
}

fn require_field(value: Option<String>, name: &'static str) -> SignerResult<String> {
    let trimmed = value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or(SignerError::MissingConfigField(name))?;
    Ok(trimmed)
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
  coinset_msp_base_url: https://api-msp.coinset.org
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
        assert_eq!(
            hex::encode(cfg.vault.launcher_id),
            "aa".repeat(32)
        );
    }
}

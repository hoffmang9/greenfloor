use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone)]
pub struct CloudWalletConfig {
    pub base_url: String,
    pub user_key_id: String,
    pub private_key_pem_path: PathBuf,
    pub vault_id: String,
    pub kms_key_id: String,
    pub kms_region: String,
    pub kms_public_key_hex: Option<String>,
    pub network: String,
}

#[derive(Debug, Deserialize)]
struct ProgramYaml {
    app: Option<AppSection>,
    cloud_wallet: Option<CloudWalletSection>,
}

#[derive(Debug, Deserialize)]
struct AppSection {
    network: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CloudWalletSection {
    base_url: Option<String>,
    user_key_id: Option<String>,
    private_key_pem_path: Option<String>,
    vault_id: Option<String>,
    kms_key_id: Option<String>,
    kms_region: Option<String>,
    kms_public_key_hex: Option<String>,
}

pub fn load_cloud_wallet_config(path: &Path) -> SignerResult<CloudWalletConfig> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!("failed to read config {}: {err}", path.display()))
    })?;
    let parsed: ProgramYaml = serde_yaml::from_str(&raw).map_err(|err| {
        SignerError::Other(format!("failed to parse config {}: {err}", path.display()))
    })?;
    let cloud_wallet = parsed
        .cloud_wallet
        .ok_or(SignerError::MissingCloudWalletField("cloud_wallet"))?;
    let network = parsed
        .app
        .and_then(|app| app.network)
        .unwrap_or_else(|| "mainnet".to_string());

    let base_url = require_field(cloud_wallet.base_url, "cloud_wallet.base_url")?;
    let user_key_id = require_field(cloud_wallet.user_key_id, "cloud_wallet.user_key_id")?;
    let private_key_pem_path = require_field(
        cloud_wallet.private_key_pem_path,
        "cloud_wallet.private_key_pem_path",
    )?;
    let vault_id = require_field(cloud_wallet.vault_id, "cloud_wallet.vault_id")?;
    let kms_key_id = require_field(cloud_wallet.kms_key_id, "cloud_wallet.kms_key_id")?;

    let pem_path = expand_path(&private_key_pem_path)?;
    validate_pem_path(&pem_path)?;

    let kms_region = cloud_wallet
        .kms_region
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "us-west-2".to_string());

    let kms_public_key_hex = cloud_wallet
        .kms_public_key_hex
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Ok(CloudWalletConfig {
        base_url: base_url.trim().trim_end_matches('/').to_string(),
        user_key_id: user_key_id.trim().to_string(),
        private_key_pem_path: pem_path,
        vault_id: vault_id.trim().to_string(),
        kms_key_id: kms_key_id.trim().to_string(),
        kms_region,
        kms_public_key_hex,
        network: network.trim().to_string(),
    })
}

fn require_field(value: Option<String>, name: &'static str) -> SignerResult<String> {
    let trimmed = value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or(SignerError::MissingCloudWalletField(name))?;
    Ok(trimmed)
}

fn expand_path(raw: &str) -> SignerResult<PathBuf> {
    let expanded = if raw.starts_with('~') {
        let home = dirs_home().ok_or_else(|| {
            SignerError::Other("failed to resolve home directory for config path".to_string())
        })?;
        PathBuf::from(raw.replacen('~', &home.to_string_lossy(), 1))
    } else {
        PathBuf::from(raw)
    };
    Ok(expanded.canonicalize().unwrap_or(expanded))
}

fn validate_pem_path(path: &Path) -> SignerResult<()> {
    if !path
        .components()
        .any(|component| component.as_os_str() == ".greenfloor")
    {
        return Err(SignerError::PemPathNotUnderDotGreenfloor);
    }
    if !path.is_file() {
        return Err(SignerError::PemPathNotFound(path.display().to_string()));
    }
    Ok(())
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_pem_outside_dot_greenfloor() {
        let err = validate_pem_path(Path::new("/tmp/not-greenfloor/key.pem")).unwrap_err();
        assert!(matches!(err, SignerError::PemPathNotUnderDotGreenfloor));
    }
}

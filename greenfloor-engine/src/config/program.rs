use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::coinset::is_xch_like_asset;
use crate::error::{SignerError, SignerResult};
use crate::hex::is_hex_id;

const DEFAULT_DEXIE_API_BASE: &str = "https://api.dexie.space";
const DEFAULT_SPLASH_API_BASE: &str = "http://john-deere.hoffmang.com:4000";
const DEFAULT_HOME_DIR: &str = "~/.greenfloor";

#[derive(Debug, Clone)]
pub struct ManagerProgramConfig {
    pub network: String,
    pub home_dir: PathBuf,
    pub dexie_api_base: String,
    pub splash_api_base: String,
    pub offer_publish_venue: String,
    pub coin_ops_minimum_fee_mojos: u64,
    pub runtime_offer_bootstrap_wait_timeout_seconds: u64,
}

#[derive(Debug, Deserialize)]
struct ProgramYaml {
    app: Option<AppYaml>,
    runtime: Option<RuntimeYaml>,
    venues: Option<VenuesYaml>,
    coin_ops: Option<CoinOpsYaml>,
    signer: Option<SignerPresence>,
    vault: Option<VaultPresence>,
}

#[derive(Debug, Deserialize)]
struct AppYaml {
    network: Option<String>,
    home_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RuntimeYaml {
    offer_bootstrap_wait_timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct VenuesYaml {
    dexie: Option<VenueBaseYaml>,
    splash: Option<VenueBaseYaml>,
    offer_publish: Option<OfferPublishYaml>,
}

#[derive(Debug, Deserialize)]
struct VenueBaseYaml {
    api_base: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OfferPublishYaml {
    provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CoinOpsYaml {
    minimum_fee_mojos: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SignerPresence {
    kms_key_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VaultPresence {
    launcher_id: Option<String>,
}

pub fn load_program_config(path: &Path) -> SignerResult<ManagerProgramConfig> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!("failed to read config {}: {err}", path.display()))
    })?;
    let parsed: ProgramYaml = serde_yaml::from_str(&raw).map_err(|err| {
        SignerError::Other(format!("failed to parse config {}: {err}", path.display()))
    })?;

    let app = parsed.app.unwrap_or(AppYaml {
        network: None,
        home_dir: None,
    });
    let network = app
        .network
        .unwrap_or_else(|| "mainnet".to_string())
        .trim()
        .to_string();
    let home_dir = expand_home_dir(
        app.home_dir
            .unwrap_or_else(|| DEFAULT_HOME_DIR.to_string())
            .trim(),
    );

    let venues = parsed.venues.unwrap_or(VenuesYaml {
        dexie: None,
        splash: None,
        offer_publish: None,
    });
    let dexie_api_base = venues
        .dexie
        .and_then(|section| section.api_base)
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_DEXIE_API_BASE.to_string());
    let splash_api_base = venues
        .splash
        .and_then(|section| section.api_base)
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_SPLASH_API_BASE.to_string());
    let offer_publish_venue = venues
        .offer_publish
        .and_then(|section| section.provider)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "dexie".to_string());
    if offer_publish_venue != "dexie" && offer_publish_venue != "splash" {
        return Err(SignerError::Other(
            "venues.offer_publish.provider must be dexie or splash".to_string(),
        ));
    }

    let coin_ops = parsed.coin_ops.unwrap_or(CoinOpsYaml {
        minimum_fee_mojos: None,
    });
    let coin_ops_minimum_fee_mojos = coin_ops.minimum_fee_mojos.unwrap_or(10_000_000);

    let runtime = parsed.runtime.unwrap_or(RuntimeYaml {
        offer_bootstrap_wait_timeout_seconds: None,
    });
    let runtime_offer_bootstrap_wait_timeout_seconds = runtime
        .offer_bootstrap_wait_timeout_seconds
        .unwrap_or(120)
        .max(10);

    Ok(ManagerProgramConfig {
        network,
        home_dir,
        dexie_api_base,
        splash_api_base,
        offer_publish_venue,
        coin_ops_minimum_fee_mojos,
        runtime_offer_bootstrap_wait_timeout_seconds,
    })
}

pub fn require_signer_offer_path(path: &Path) -> SignerResult<()> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!("failed to read config {}: {err}", path.display()))
    })?;
    let parsed: ProgramYaml = serde_yaml::from_str(&raw).map_err(|err| {
        SignerError::Other(format!("failed to parse config {}: {err}", path.display()))
    })?;
    let kms_key_id = parsed
        .signer
        .and_then(|signer| signer.kms_key_id)
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let launcher_id = parsed
        .vault
        .and_then(|vault| vault.launcher_id)
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    if kms_key_id.is_empty() || launcher_id.is_empty() {
        return Err(SignerError::Other(
            "offer execution requires signer.kms_key_id and vault.launcher_id in program config"
                .to_string(),
        ));
    }
    Ok(())
}

pub fn is_testnet_network(network: &str) -> bool {
    matches!(
        network.trim().to_ascii_lowercase().as_str(),
        "testnet" | "testnet11"
    )
}

pub fn resolve_trade_asset_for_network(asset: &str, network: &str) -> String {
    let normalized = asset.trim().to_ascii_lowercase();
    if is_xch_like_asset(&normalized) {
        if is_testnet_network(network) {
            "txch".to_string()
        } else {
            "xch".to_string()
        }
    } else if is_hex_id(&normalized) {
        normalized
    } else {
        asset.trim().to_string()
    }
}

pub fn resolve_quote_asset_for_offer(quote_asset: &str, network: &str) -> String {
    resolve_trade_asset_for_network(quote_asset, network)
}

pub fn resolve_dexie_base_url(
    network: &str,
    explicit: Option<&str>,
    program_base: &str,
) -> SignerResult<String> {
    if let Some(url) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(url.trim_end_matches('/').to_string());
    }
    if is_testnet_network(network) {
        return Ok("https://api-testnet.dexie.space".to_string());
    }
    Ok(program_base.trim().trim_end_matches('/').to_string())
}

pub fn resolve_splash_base_url(explicit: Option<&str>, program_base: &str) -> String {
    explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .unwrap_or_else(|| program_base.trim().trim_end_matches('/').to_string())
}

pub fn resolve_offer_publish_settings(
    program: &ManagerProgramConfig,
    network: &str,
    venue_override: Option<&str>,
    dexie_base_url: Option<&str>,
    splash_base_url: Option<&str>,
) -> SignerResult<(String, String, String)> {
    let venue = venue_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| program.offer_publish_venue.clone());
    if venue != "dexie" && venue != "splash" {
        return Err(SignerError::Other(
            "offer publish venue must be dexie or splash".to_string(),
        ));
    }
    let dexie_base = resolve_dexie_base_url(network, dexie_base_url, &program.dexie_api_base)?;
    let splash_base = resolve_splash_base_url(splash_base_url, &program.splash_api_base);
    Ok((venue, dexie_base, splash_base))
}

pub fn action_side_from_pricing(pricing: &Value) -> String {
    pricing
        .get("side")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("sell")
        .to_string()
}

fn expand_home_dir(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    if path == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_testnet_dexie_default() {
        let url = resolve_dexie_base_url("testnet11", None, "https://api.dexie.space").expect("url");
        assert_eq!(url, "https://api-testnet.dexie.space");
    }

    #[test]
    fn maps_xch_to_txch_on_testnet() {
        assert_eq!(resolve_quote_asset_for_offer("xch", "testnet11"), "txch");
        assert_eq!(resolve_quote_asset_for_offer("xch", "mainnet"), "xch");
    }
}

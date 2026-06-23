use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::keys_registry::SignerKeyEntry;
use super::signer::{parse_signer_config, SignerConfig};
use crate::coinset::is_xch_like_asset;
use crate::error::{SignerError, SignerResult};
use crate::hex::is_hex_id;
use crate::paths::expand_home;

pub(crate) const DEFAULT_DEXIE_API_BASE: &str = "https://api.dexie.space";
pub(crate) const DEFAULT_SPLASH_API_BASE: &str = "http://john-deere.hoffmang.com:4000";
const DEFAULT_HOME_DIR: &str = "~/.greenfloor";

#[derive(Debug, Clone)]
pub struct ManagerProgramConfig {
    pub network: String,
    pub home_dir: PathBuf,
    pub app_log_level: String,
    pub app_log_level_was_missing: bool,
    pub dexie_api_base: String,
    pub splash_api_base: String,
    pub offer_publish_venue: String,
    pub coin_ops_minimum_fee_mojos: u64,
    pub coin_ops_max_operations_per_run: i64,
    pub coin_ops_max_daily_fee_budget_mojos: i64,
    pub coin_ops_split_fee_mojos: i64,
    pub coin_ops_combine_fee_mojos: i64,
    pub runtime_offer_bootstrap_wait_timeout_seconds: u64,
    pub runtime_market_slot_count: u64,
    pub runtime_offer_parallelism_enabled: bool,
    pub runtime_offer_parallelism_max_workers: usize,
    pub runtime_reservation_ttl_seconds: u64,
    pub runtime_dry_run: bool,
    pub runtime_loop_interval_seconds: u64,
    pub storage_audit_retention_days: u64,
    pub tx_block_trigger_mode: String,
    pub tx_block_websocket_url: String,
    pub tx_block_websocket_reconnect_interval_seconds: u64,
    pub tx_block_fallback_poll_interval_seconds: u64,
    pub signer_kms_key_id: String,
    pub signer_kms_region: String,
    pub vault_launcher_id: String,
    pub dev_python_min_version: String,
    pub signer_key_registry: HashMap<String, SignerKeyEntry>,
}

impl ManagerProgramConfig {
    #[must_use]
    pub fn signer_offer_path_configured(&self) -> bool {
        !self.signer_kms_key_id.is_empty() && !self.vault_launcher_id.is_empty()
    }

    /// Require signer offer path.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn require_signer_offer_path(&self) -> SignerResult<()> {
        if self.signer_offer_path_configured() {
            return Ok(());
        }
        Err(SignerError::SignerPathNotConfigured)
    }
}

#[derive(Debug, Clone)]
pub struct CycleProgramConfig {
    program: Box<ManagerProgramConfig>,
    signer: Option<SignerConfig>,
}

impl CycleProgramConfig {
    /// Daemon cycle load: never fail the whole cycle on signer YAML errors.
    #[must_use]
    pub fn from_parsed(program: ManagerProgramConfig, raw: &Value) -> Self {
        let signer = if program.signer_offer_path_configured() {
            parse_signer_config(raw).ok()
        } else {
            None
        };
        Self {
            program: Box::new(program),
            signer,
        }
    }

    #[must_use]
    pub fn from_parts(program: ManagerProgramConfig, signer: Option<SignerConfig>) -> Self {
        Self {
            program: Box::new(program),
            signer,
        }
    }

    #[must_use]
    pub fn program(&self) -> &ManagerProgramConfig {
        &self.program
    }

    /// Signer for execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn signer_for_execution(&self) -> SignerResult<&SignerConfig> {
        self.program.require_signer_offer_path()?;
        self.signer
            .as_ref()
            .ok_or(SignerError::MissingConfigField("signer"))
    }
}

pub const SIGNER_SKIP_NO_SIGNER_PATH: &str = "skipped_no_signer";
pub const SIGNER_SKIP_MISSING_SIGNER_CONFIG: &str = "skipped_missing_signer_config";

#[must_use]
pub fn signer_execution_skip_reason(err: &SignerError) -> String {
    match err {
        SignerError::SignerPathNotConfigured => SIGNER_SKIP_NO_SIGNER_PATH.to_string(),
        SignerError::MissingConfigField("signer") => SIGNER_SKIP_MISSING_SIGNER_CONFIG.to_string(),
        other => other.to_string(),
    }
}

#[must_use]
pub fn is_signer_execution_soft_skip(err: &SignerError) -> bool {
    matches!(
        signer_execution_skip_reason(err).as_str(),
        SIGNER_SKIP_NO_SIGNER_PATH | SIGNER_SKIP_MISSING_SIGNER_CONFIG
    )
}

impl Default for ManagerProgramConfig {
    fn default() -> Self {
        Self {
            network: "mainnet".to_string(),
            home_dir: expand_home(DEFAULT_HOME_DIR),
            app_log_level: "INFO".to_string(),
            app_log_level_was_missing: true,
            dexie_api_base: DEFAULT_DEXIE_API_BASE.to_string(),
            splash_api_base: DEFAULT_SPLASH_API_BASE.to_string(),
            offer_publish_venue: "dexie".to_string(),
            coin_ops_minimum_fee_mojos: 10_000_000,
            coin_ops_max_operations_per_run: 20,
            coin_ops_max_daily_fee_budget_mojos: 0,
            coin_ops_split_fee_mojos: 0,
            coin_ops_combine_fee_mojos: 0,
            runtime_offer_bootstrap_wait_timeout_seconds: 120,
            runtime_market_slot_count: 0,
            runtime_offer_parallelism_enabled: false,
            runtime_offer_parallelism_max_workers: 4,
            runtime_reservation_ttl_seconds: 300,
            runtime_dry_run: false,
            runtime_loop_interval_seconds: 30,
            storage_audit_retention_days: crate::storage::DEFAULT_AUDIT_RETENTION_DAYS,
            tx_block_trigger_mode: "websocket".to_string(),
            tx_block_websocket_url: "wss://api.coinset.org/ws".to_string(),
            tx_block_websocket_reconnect_interval_seconds: 30,
            tx_block_fallback_poll_interval_seconds: 60,
            signer_kms_key_id: String::new(),
            signer_kms_region: "us-west-2".to_string(),
            vault_launcher_id: String::new(),
            dev_python_min_version: String::new(),
            signer_key_registry: HashMap::new(),
        }
    }
}

pub use super::program_parse::parse_program_config;

/// Read program yaml.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn read_program_yaml(path: &Path) -> SignerResult<Value> {
    super::yaml_file::read_yaml_file_labeled(path, "config")
}

/// Load program config.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_program_config(path: &Path) -> SignerResult<ManagerProgramConfig> {
    parse_program_config(&read_program_yaml(path)?)
}

#[derive(Debug, Clone)]
pub struct ProgramConfigBundle {
    pub program: ManagerProgramConfig,
    pub signer: SignerConfig,
}

/// Program bundle from parsed.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn program_bundle_from_parsed(
    program: ManagerProgramConfig,
    raw: &Value,
) -> SignerResult<ProgramConfigBundle> {
    Ok(ProgramConfigBundle {
        program,
        signer: super::signer::parse_signer_config(raw)?,
    })
}

/// Load program bundle.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_program_bundle(path: &Path) -> SignerResult<ProgramConfigBundle> {
    let raw = read_program_yaml(path)?;
    let program = parse_program_config(&raw)?;
    program_bundle_from_parsed(program, &raw)
}

/// Program bundle gated from parsed.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn program_bundle_gated_from_parsed(
    program: ManagerProgramConfig,
    raw: &Value,
) -> SignerResult<ProgramConfigBundle> {
    program.require_signer_offer_path()?;
    program_bundle_from_parsed(program, raw)
}

/// Load program bundle gated.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_program_bundle_gated(path: &Path) -> SignerResult<ProgramConfigBundle> {
    let raw = read_program_yaml(path)?;
    let program = parse_program_config(&raw)?;
    program_bundle_gated_from_parsed(program, &raw)
}

/// Load execution bundle for coin-list; maps missing signer path to
/// [`SignerError::SignerPathNotConfigured`] for stable CLI exit handling.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_program_bundle_for_coin_list(path: &Path) -> SignerResult<ProgramConfigBundle> {
    load_program_bundle_gated(path)
}

#[must_use]
pub fn is_testnet_network(network: &str) -> bool {
    matches!(
        network.trim().to_ascii_lowercase().as_str(),
        "testnet" | "testnet11"
    )
}

#[must_use]
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

#[must_use]
pub fn resolve_quote_asset_for_offer(quote_asset: &str, network: &str) -> String {
    resolve_trade_asset_for_network(quote_asset, network)
}

/// Resolve dexie base url.
///
/// # Errors
///
/// Returns an error if the operation fails.
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
        .map_or_else(
            || program_base.trim().trim_end_matches('/').to_string(),
            |value| value.trim_end_matches('/').to_string(),
        )
}

/// Resolve offer publish settings.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn resolve_offer_publish_settings(
    program: &ManagerProgramConfig,
    network: &str,
    venue_override: Option<&str>,
    dexie_base_url: Option<&str>,
    splash_base_url: Option<&str>,
) -> SignerResult<(String, String, String)> {
    let venue = match venue_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value.to_ascii_lowercase(),
        None => program.offer_publish_venue.clone(),
    };
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

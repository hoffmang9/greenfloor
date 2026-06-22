//! Section parsers for `program.yaml` (extracted from `program.rs`).

mod coin_ops;
mod guards;
mod helpers;
mod runtime;
mod signer_vault;
mod tx_block;
mod venue;

use serde_json::Value;

use coin_ops::parse_coin_ops_config;
use guards::{parse_dev_python_min_version, reject_cloud_wallet, require_pushover_provider};
use runtime::parse_runtime_config;
use signer_vault::parse_signer_vault_ids;
use tx_block::parse_tx_block_config;
use venue::parse_venue_config;

use super::keys_registry::parse_signer_key_registry;
use super::program::ManagerProgramConfig;
use super::yaml_fields::{req_mapping, req_mapping_from_map, req_str};
use crate::error::SignerResult;
use crate::file_logging::normalize_log_level_string;
use crate::paths::expand_home;

/// Parse program config.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn parse_program_config(raw: &Value) -> SignerResult<ManagerProgramConfig> {
    reject_cloud_wallet(raw)?;

    let app = req_mapping(raw, "app")?;
    let runtime_section = req_mapping(raw, "runtime")?;
    let chain_signals = req_mapping(raw, "chain_signals")?;
    let dev = req_mapping(raw, "dev")?;
    let dev_python_min_version = parse_dev_python_min_version(dev)?;
    require_pushover_provider(raw)?;

    let tx_trigger = req_mapping_from_map(chain_signals, "tx_block_trigger")?;
    let coin_ops_section = raw.get("coin_ops").and_then(Value::as_object);
    let network = req_str(app, "network")?;
    let venue = parse_venue_config(raw)?;
    let coin_ops = parse_coin_ops_config(coin_ops_section)?;
    let runtime = parse_runtime_config(runtime_section)?;
    let tx_block = parse_tx_block_config(tx_trigger, &network)?;
    let signer_vault = parse_signer_vault_ids(raw);

    let app_log_level_was_missing = !app.contains_key("log_level");
    let app_log_level = normalize_log_level_string(
        app.get("log_level")
            .and_then(Value::as_str)
            .unwrap_or("INFO"),
    );

    Ok(ManagerProgramConfig {
        network,
        home_dir: expand_home(req_str(app, "home_dir")?.trim()),
        app_log_level,
        app_log_level_was_missing,
        dev_python_min_version,
        signer_key_registry: parse_signer_key_registry(raw)?,
        dexie_api_base: venue.dexie_api_base,
        splash_api_base: venue.splash_api_base,
        offer_publish_venue: venue.offer_publish_venue,
        coin_ops_minimum_fee_mojos: coin_ops.coin_ops_minimum_fee_mojos,
        coin_ops_max_operations_per_run: coin_ops.coin_ops_max_operations_per_run,
        coin_ops_max_daily_fee_budget_mojos: coin_ops.coin_ops_max_daily_fee_budget_mojos,
        coin_ops_split_fee_mojos: coin_ops.coin_ops_split_fee_mojos,
        coin_ops_combine_fee_mojos: coin_ops.coin_ops_combine_fee_mojos,
        runtime_offer_bootstrap_wait_timeout_seconds: runtime
            .runtime_offer_bootstrap_wait_timeout_seconds,
        runtime_market_slot_count: runtime.runtime_market_slot_count,
        runtime_offer_parallelism_enabled: runtime.runtime_offer_parallelism_enabled,
        runtime_offer_parallelism_max_workers: runtime.runtime_offer_parallelism_max_workers,
        runtime_reservation_ttl_seconds: runtime.runtime_reservation_ttl_seconds,
        runtime_dry_run: runtime.runtime_dry_run,
        runtime_loop_interval_seconds: runtime.runtime_loop_interval_seconds,
        tx_block_trigger_mode: tx_block.tx_block_trigger_mode,
        tx_block_websocket_url: tx_block.tx_block_websocket_url,
        tx_block_websocket_reconnect_interval_seconds: tx_block
            .tx_block_websocket_reconnect_interval_seconds,
        tx_block_fallback_poll_interval_seconds: tx_block.tx_block_fallback_poll_interval_seconds,
        signer_kms_key_id: signer_vault.signer_kms_key_id,
        signer_kms_region: signer_vault.signer_kms_region,
        vault_launcher_id: signer_vault.vault_launcher_id,
    })
}

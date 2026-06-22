use serde_json::json;

use crate::config::{load_markets_config_with_overlay, load_program_config};
use crate::error::SignerResult;
use crate::hex::{is_hex_id, normalize_hex_id};
use crate::manager_cli::cats_catalog::load_cats_catalog;
use crate::manager_cli::context::ManagerContext;

pub fn run_program_fields(ctx: &ManagerContext) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let keys_registry: serde_json::Map<_, _> = program
        .signer_key_registry
        .iter()
        .map(|(key_id, entry)| {
            (
                key_id.clone(),
                json!({
                    "key_id": key_id,
                    "fingerprint": entry.fingerprint,
                    "network": entry.network,
                    "keyring_yaml_path": entry.keyring_yaml_path,
                }),
            )
        })
        .collect();
    ctx.emit_json(&json!({
        "network": program.network,
        "home_dir": program.home_dir.display().to_string(),
        "signer_kms_key_id": program.signer_kms_key_id,
        "signer_kms_region": program.signer_kms_region,
        "vault_launcher_id": program.vault_launcher_id,
        "signer_offer_path_configured": program.signer_offer_path_configured(),
        "dev_python_min_version": program.dev_python_min_version,
        "keys_registry": keys_registry,
    }))?;
    Ok(0)
}

pub fn run_markets_fields(ctx: &ManagerContext) -> SignerResult<i32> {
    let markets =
        load_markets_config_with_overlay(&ctx.markets_config, ctx.testnet_markets_path())?;
    let all: Vec<_> = markets.markets.iter().map(market_fields_row).collect();
    let enabled: Vec<_> = markets
        .markets
        .iter()
        .filter(|market| market.enabled)
        .map(market_fields_row)
        .collect();
    ctx.emit_json(&json!({
        "markets_config": ctx.markets_config.display().to_string(),
        "markets": all,
        "enabled_markets": enabled,
    }))?;
    Ok(0)
}

pub fn run_cats_fields(ctx: &ManagerContext) -> SignerResult<i32> {
    let catalog = load_cats_catalog(&ctx.cats_config)?;
    let symbol_to_asset_id = build_symbol_to_asset_id(&catalog);
    ctx.emit_json(&json!({
        "cats_config": ctx.cats_config.display().to_string(),
        "symbol_to_asset_id": symbol_to_asset_id,
        "cats": catalog,
    }))?;
    Ok(0)
}

fn market_fields_row(market: &crate::config::MarketConfig) -> serde_json::Value {
    json!({
        "id": market.market_id,
        "enabled": market.enabled,
        "base_asset": market.base_asset,
        "base_symbol": market.base_symbol,
        "quote_asset": market.quote_asset,
        "quote_asset_type": market.quote_asset_type,
        "receive_address": market.receive_address,
        "signer_key_id": market.signer_key_id,
        "mode": market.mode,
    })
}

fn build_symbol_to_asset_id(
    catalog: &[serde_json::Value],
) -> serde_json::Map<String, serde_json::Value> {
    catalog
        .iter()
        .filter_map(|row| {
            let symbol = row
                .get("base_symbol")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            let asset_id = row
                .get("asset_id")
                .and_then(serde_json::Value::as_str)
                .map(normalize_hex_id)
                .filter(|value| is_hex_id(value))?;
            Some((symbol.to_ascii_lowercase(), json!(asset_id)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager_cli::test_support::{pop_json, repo_root, ManagerContextBuilder};

    #[test]
    fn program_fields_reads_example_program() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
        let harness = ManagerContextBuilder::new(program, dir.path().join("unused-markets.yaml"))
            .cats_config(dir.path().join("unused-cats.yaml"))
            .build_capturing();
        let code = run_program_fields(&harness.ctx).expect("program-fields");
        assert_eq!(code, 0);
        let payload = pop_json(&harness.captured);
        assert_eq!(
            payload.get("network").and_then(serde_json::Value::as_str),
            Some("mainnet")
        );
        let registry = payload
            .get("keys_registry")
            .and_then(serde_json::Value::as_object)
            .expect("keys registry");
        assert!(registry.contains_key("key-main-1"));
    }

    #[test]
    fn markets_fields_reads_example_markets() {
        let harness = ManagerContextBuilder::new(
            repo_root().join("config/program.yaml"),
            repo_root().join("config/markets.yaml"),
        )
        .cats_config(repo_root().join("config/cats.yaml"))
        .testnet_markets(repo_root().join("config/testnet-markets.yaml"))
        .build_capturing();
        let code = run_markets_fields(&harness.ctx).expect("markets-fields");
        assert_eq!(code, 0);
        let payload = pop_json(&harness.captured);
        let enabled = payload
            .get("enabled_markets")
            .and_then(|v| v.as_array())
            .expect("enabled markets");
        assert!(!enabled.is_empty());
        assert!(enabled.iter().all(|row| {
            row.get("enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        }));
    }

    #[test]
    fn cats_fields_reads_example_cats() {
        let harness = ManagerContextBuilder::new(
            repo_root().join("config/program.yaml"),
            repo_root().join("config/markets.yaml"),
        )
        .cats_config(repo_root().join("config/cats.yaml"))
        .build_capturing();
        let code = run_cats_fields(&harness.ctx).expect("cats-fields");
        assert_eq!(code, 0);
        let payload = pop_json(&harness.captured);
        let symbol_map = payload
            .get("symbol_to_asset_id")
            .and_then(serde_json::Value::as_object)
            .expect("symbol_to_asset_id map");
        assert!(!symbol_map.is_empty());
    }
}

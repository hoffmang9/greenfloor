use std::collections::BTreeMap;

use serde_json::json;

use crate::coin_ops::compute_bucket_counts_from_coins;
use crate::config::load_signer_config;
use crate::config::MarketConfig;
use crate::cycle::MarketCycleResultState;
use crate::error::SignerResult;
use crate::hex::{default_mojo_multiplier_for_asset, is_hex_id, normalize_hex_id};
use crate::offer::resolve_offer_assets_for_action;
use crate::storage::SqliteStore;

use super::coinset_spendable::list_spendable_base_unit_amounts;
use super::market_context::DaemonCycleResources;

/// When `market.base_asset` is a hex CAT id, it must match the signer-resolved id used for coinset.
pub fn assert_inventory_asset_resolution_matches_config(
    market: &MarketConfig,
    resolved_base_asset_id: &str,
) -> SignerResult<()> {
    let configured = market.base_asset.trim();
    if !is_hex_id(configured) {
        return Ok(());
    }
    let config_hex = normalize_hex_id(configured);
    let resolved_hex = normalize_hex_id(resolved_base_asset_id);
    if resolved_hex.is_empty() {
        return Err(crate::error::SignerError::Other(format!(
            "inventory_asset_resolution_invalid: market {} configured_base_asset={config_hex} resolved_base_asset={resolved_base_asset_id}",
            market.market_id
        )));
    }
    if config_hex != resolved_hex {
        return Err(crate::error::SignerError::Other(format!(
            "inventory_asset_resolution_mismatch: market {} configured_base_asset={config_hex} resolved_base_asset={resolved_hex}",
            market.market_id
        )));
    }
    Ok(())
}

pub async fn run_inventory_phase(
    store: &SqliteStore,
    resources: &DaemonCycleResources,
    market: &MarketConfig,
    state: &mut MarketCycleResultState,
) -> SignerResult<BTreeMap<i64, i64>> {
    let ladder_sizes: Vec<i64> = market
        .ladders
        .get("sell")
        .into_iter()
        .flat_map(|entries| entries.iter().map(|entry| entry.size_base_units))
        .filter(|size| *size > 0)
        .collect();
    if ladder_sizes.is_empty() {
        return Ok(BTreeMap::new());
    }

    if !resources.signer_offer_path_configured {
        store.add_audit_event(
            "inventory_bucket_scan",
            &json!({
                "market_id": market.market_id,
                "source": "skipped_no_signer",
                "bucket_counts": {},
            }),
            Some(&market.market_id),
        )?;
        return Ok(BTreeMap::new());
    }

    let base_unit_multiplier = default_mojo_multiplier_for_asset(market.base_asset.trim());
    let scan_result: SignerResult<(String, usize, BTreeMap<i64, i64>)> = async {
        let signer_config = load_signer_config(resources.program_path())?;
        let (resolved_base_asset_id, _) =
            resolve_offer_assets_for_action(&signer_config, market.base_asset.trim(), "xch")
                .await?;
        assert_inventory_asset_resolution_matches_config(market, &resolved_base_asset_id)?;
        let amounts = list_spendable_base_unit_amounts(
            &resources.network,
            &market.receive_address,
            &resolved_base_asset_id,
            base_unit_multiplier,
        )
        .await?;
        let bucket_counts = compute_bucket_counts_from_coins(&amounts, &ladder_sizes);
        Ok((resolved_base_asset_id, amounts.len(), bucket_counts))
    }
    .await;

    match scan_result {
        Ok((resolved_base_asset_id, coin_count, bucket_counts)) => {
            store.add_audit_event(
                "inventory_bucket_scan",
                &json!({
                    "market_id": market.market_id,
                    "source": "coinset",
                    "resolved_asset_id": resolved_base_asset_id,
                    "coin_count": coin_count,
                    "bucket_counts": bucket_counts,
                }),
                Some(&market.market_id),
            )?;
            Ok(bucket_counts)
        }
        Err(err) => {
            state.record_phase_error();
            store.add_audit_event(
                "inventory_bucket_scan_error",
                &json!({"market_id": market.market_id, "error": err.to_string()}),
                Some(&market.market_id),
            )?;
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn sample_market(base_asset: &str) -> MarketConfig {
        MarketConfig {
            market_id: "m1".to_string(),
            enabled: true,
            base_asset: base_asset.to_string(),
            base_symbol: "AS1".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "unstable".to_string(),
            receive_address: "xch1test".to_string(),
            signer_key_id: "key-main-1".to_string(),
            mode: "sell_only".to_string(),
            pricing: json!({}),
            cancel_move_threshold_bps: None,
            ladders: HashMap::new(),
        }
    }

    #[test]
    fn hex_config_accepts_matching_resolved_asset() {
        let asset = "a".repeat(64);
        let market = sample_market(&asset);
        assert_inventory_asset_resolution_matches_config(&market, &asset).expect("match");
        assert_inventory_asset_resolution_matches_config(&market, &format!("0x{asset}"))
            .expect("0x prefix");
    }

    #[test]
    fn hex_config_rejects_resolved_mismatch() {
        let configured = "a".repeat(64);
        let resolved = "b".repeat(64);
        let market = sample_market(&configured);
        let err = assert_inventory_asset_resolution_matches_config(&market, &resolved)
            .expect_err("mismatch");
        assert!(err
            .to_string()
            .contains("inventory_asset_resolution_mismatch"));
    }

    #[test]
    fn non_hex_config_skips_resolution_match_check() {
        let market = sample_market("my-cat-ticker");
        assert_inventory_asset_resolution_matches_config(&market, "c".repeat(64).as_str())
            .expect("symbol config uses resolved id only");
    }
}

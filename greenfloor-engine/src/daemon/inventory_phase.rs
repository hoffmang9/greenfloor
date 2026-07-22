use std::collections::BTreeMap;

use serde_json::json;
use tracing::Level;

use crate::coin_ops::compute_bucket_counts_from_coins;
use crate::config::{is_signer_execution_soft_skip, signer_execution_skip_reason, MarketConfig};
use crate::cycle::MarketCycleResultState;
use crate::error::SignerResult;
use crate::hex::{default_mojo_multiplier_for_asset, is_hex_id, normalize_hex_id};
use crate::operator_log::{LogContext, INVENTORY_BUCKET_SCAN, INVENTORY_BUCKET_SCAN_ERROR};
use crate::storage::SqliteStore;

use super::coinset_spendable::list_spendable_base_unit_amounts_for_signer;
use super::market_context::DaemonCycleResources;

/// When `market.base_asset` is a hex CAT id, it must match the signer-resolved id used for coinset.
///
/// # Errors
///
/// Returns an error when the configured hex id does not match the resolved asset id.
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

/// Scan spendable inventory into ladder bucket counts, skipping HTTP when WS freshness allows.
///
/// # Errors
///
/// Returns an error when Coinset inventory scanning or audit persistence fails.
#[allow(clippy::too_many_lines)]
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
        return Ok(BTreeMap::default());
    }

    if !resources.coinset.inventory_freshness.needs_refresh(
        &market.market_id,
        super::inventory_freshness::INVENTORY_MAX_STALENESS,
    ) {
        if let Some(cached) = resources
            .coinset
            .inventory_freshness
            .cached_buckets(&market.market_id)
        {
            LogContext::MARKET_CYCLE.dual_audit(
                store,
                Level::DEBUG,
                "inventory bucket scan skipped (fresh)",
                INVENTORY_BUCKET_SCAN,
                &json!({
                    "market_id": market.market_id,
                    "source": "coinset_ws_fresh",
                    "bucket_counts": cached,
                }),
                Some(&market.market_id),
            )?;
            return Ok(cached);
        }
    }

    let base_unit_multiplier = default_mojo_multiplier_for_asset(market.base_asset.trim());
    let scan_result: SignerResult<(String, usize, BTreeMap<i64, i64>)> = async {
        let resolver = resources.asset_resolver()?;
        let resolved_base_asset_id = resolver.resolve_base(market.base_asset.trim()).await?;
        assert_inventory_asset_resolution_matches_config(market, &resolved_base_asset_id)?;
        let signer_config = resources.signer_for_execution()?;
        let amounts = list_spendable_base_unit_amounts_for_signer(
            &resources.network,
            signer_config,
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
            resources
                .coinset
                .inventory_freshness
                .mark_fresh(&market.market_id, bucket_counts.clone());
            LogContext::MARKET_CYCLE.dual_audit(
                store,
                Level::DEBUG,
                "inventory bucket scan",
                INVENTORY_BUCKET_SCAN,
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
        Err(err) if is_signer_execution_soft_skip(&err) => {
            LogContext::MARKET_CYCLE.dual_audit(
                store,
                Level::DEBUG,
                "inventory bucket scan skipped",
                INVENTORY_BUCKET_SCAN,
                &json!({
                    "market_id": market.market_id,
                    "source": signer_execution_skip_reason(&err),
                    "bucket_counts": {},
                }),
                Some(&market.market_id),
            )?;
            Ok(BTreeMap::default())
        }
        Err(err) => {
            state.record_phase_error();
            LogContext::MARKET_CYCLE.dual_audit(
                store,
                Level::WARN,
                "inventory bucket scan failed",
                INVENTORY_BUCKET_SCAN_ERROR,
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
            ladders: HashMap::default(),
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

    #[test]
    fn freshness_cache_preserves_last_buckets() {
        let freshness = super::super::inventory_freshness::InventoryFreshnessCache::new();
        let buckets = BTreeMap::from([(1, 3), (10, 1)]);
        freshness.mark_fresh("m1", buckets.clone());
        assert!(!freshness.needs_refresh(
            "m1",
            super::super::inventory_freshness::INVENTORY_MAX_STALENESS
        ));
        assert_eq!(freshness.cached_buckets("m1"), Some(buckets));
    }

    #[tokio::test]
    async fn run_inventory_phase_skips_http_when_fresh_and_rescans_after_stale() {
        use crate::adapters::DexieClient;
        use crate::config::LadderEntry;
        use crate::config::{
            empty_cat_ticker_index, CycleProgramConfig, ManagerProgramConfig, MarketsConfig,
        };
        use crate::cycle::MarketCycleResultState;
        use crate::daemon::coinset_ws::{CoinsetWsShared, InventoryP2Index};
        use crate::daemon::cycle_paths::DaemonCyclePaths;
        use crate::daemon::market_context::DaemonCycleResources;
        use crate::operator_log::INVENTORY_BUCKET_SCAN;
        use std::path::PathBuf;
        use std::sync::Arc;
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let buckets = BTreeMap::from([(10, 2)]);
        let freshness = crate::daemon::InventoryFreshnessCache::new();
        freshness.mark_fresh("m1", buckets.clone());
        let coinset = CoinsetWsShared::new(
            Arc::new(InventoryP2Index::default()),
            Arc::clone(&freshness),
        );
        let mut market = sample_market("xch");
        market.ladders.insert(
            "sell".to_string(),
            vec![LadderEntry {
                size_base_units: 10,
                target_count: 1,
                split_buffer_count: 0,
                combine_when_excess_factor: 2.0,
            }],
        );
        let resources = DaemonCycleResources::with_program_config(
            CycleProgramConfig::from_parts(ManagerProgramConfig::default(), None),
            MarketsConfig {
                markets: vec![market.clone()],
            },
            "mainnet".to_string(),
            DexieClient::new("https://api.dexie.space"),
            DaemonCyclePaths::new(
                PathBuf::from("/tmp/program.yaml"),
                PathBuf::from("/tmp/markets.yaml"),
                None,
            ),
            coinset,
            empty_cat_ticker_index(),
        );
        let mut state = MarketCycleResultState::default();
        let returned = run_inventory_phase(&store, &resources, &market, &mut state)
            .await
            .expect("fresh skip");
        assert_eq!(returned, buckets);
        let audits = store
            .list_recent_audit_events(Some(&[INVENTORY_BUCKET_SCAN]), Some("m1"), 5)
            .expect("audits");
        assert!(audits.iter().any(|event| {
            event.payload.get("source").and_then(|value| value.as_str()) == Some("coinset_ws_fresh")
        }));

        freshness.mark_stale("m1");
        assert!(freshness.needs_refresh(
            "m1",
            super::super::inventory_freshness::INVENTORY_MAX_STALENESS
        ));
        // After stale, phase attempts HTTP scan; without a live signer this soft-skips
        // to empty buckets rather than returning the cached fresh map.
        let mut state = MarketCycleResultState::default();
        let after_stale = run_inventory_phase(&store, &resources, &market, &mut state)
            .await
            .expect("stale path");
        assert_ne!(
            after_stale, buckets,
            "stale path must not return the prior fresh-skip cache"
        );
    }
}

use crate::coinset::get_conservative_fee_estimate_for_signer;
use crate::config::{
    action_side_from_pricing, load_gated_operator_market, resolve_offer_publish_settings,
    GatedOperatorMarket, GatedOperatorMarketLoadRequest, OperatorMarketCommand,
};
use crate::error::SignerResult;
use crate::offer::build_context::resolve_quote_price_for_pricing;
use crate::offer::{normalize_offer_side, ResolvedMarketOfferAssets};

use super::BuildAndPostOfferRequest;
use crate::offer::operator::logging::{sync_manager_file_logging, warn_if_log_level_auto_healed};
#[cfg(test)]
use crate::offer::operator::test_overrides::BuildOfferTestOverrides;

#[derive(Debug, Clone)]
pub(crate) struct ResolvedBuildAndPostContext {
    pub gated: GatedOperatorMarket,
    pub publish_venue: String,
    pub dexie_base_url: String,
    pub splash_base_url: String,
    pub offer_assets: ResolvedMarketOfferAssets,
    action_side_override: Option<String>,
    pub offer_fee_mojos: u64,
    pub offer_fee_source: String,
    #[cfg(test)]
    pub test_overrides: BuildOfferTestOverrides,
}

impl ResolvedBuildAndPostContext {
    #[must_use]
    pub(crate) fn action_side(&self) -> String {
        resolve_action_side(
            self.action_side_override.as_deref(),
            &self.gated.market_row.pricing,
        )
    }

    /// Quote-per-base from market pricing.
    ///
    /// # Errors
    ///
    /// Returns an error when pricing lacks a usable quote price.
    pub(crate) fn quote_price(&self) -> SignerResult<f64> {
        resolve_quote_price_for_pricing(&self.gated.market_row.pricing)
    }
}

pub(super) async fn resolve_build_and_post_context(
    request: &BuildAndPostOfferRequest,
) -> SignerResult<ResolvedBuildAndPostContext> {
    let gated = load_gated_operator_market(&GatedOperatorMarketLoadRequest {
        program_path: &request.program_path,
        markets_path: &request.markets_path,
        testnet_markets_path: request.testnet_markets_path.as_deref(),
        cats_path: request.cats_path.as_deref(),
        network: &request.network,
        market_id: request.market_id.as_deref(),
        pair: request.pair.as_deref(),
        command: OperatorMarketCommand::Build,
    })?;
    sync_manager_file_logging(&gated.program.home_dir, &gated.program.app_log_level)?;
    warn_if_log_level_auto_healed(
        gated.program.app_log_level_was_missing,
        &request.program_path,
    );
    let (publish_venue, dexie_base_url, splash_base_url) = resolve_offer_publish_settings(
        &gated.program,
        &request.network,
        request.publish_venue.as_deref(),
        request.dexie_base_url.as_deref(),
        request.splash_base_url.as_deref(),
    )?;
    let resolver = gated.asset_resolver();
    let assets = resolver.resolve_market_assets(&gated.market_row).await?;
    let action_side_override = request
        .action_side
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let (offer_fee_mojos, offer_fee_source) =
        resolve_maker_offer_fee(&gated.signer, &gated.operator_network).await;

    let ctx = ResolvedBuildAndPostContext {
        gated,
        publish_venue,
        dexie_base_url,
        splash_base_url,
        offer_assets: assets,
        action_side_override,
        offer_fee_mojos,
        offer_fee_source,
        #[cfg(test)]
        test_overrides: request.test_overrides.clone(),
    };
    ctx.quote_price()?;
    Ok(ctx)
}

pub(super) fn resolve_action_side(
    action_side_override: Option<&str>,
    pricing: &serde_json::Value,
) -> String {
    if let Some(side) = action_side_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return normalize_offer_side(side).to_string();
    }
    action_side_from_pricing(pricing)
}

async fn resolve_maker_offer_fee(
    signer: &crate::config::SignerConfig,
    operator_network: &str,
) -> (u64, String) {
    match get_conservative_fee_estimate_for_signer(signer, operator_network, 1_000_000, Some(1))
        .await
    {
        Ok(Some(fee)) => (fee, "coinset_conservative_fee".to_string()),
        Ok(None) | Err(_) => (0, "coinset_fee_unavailable".to_string()),
    }
}

#[cfg(test)]
pub(crate) fn signer_denomination_test_context(
    program: crate::config::ManagerProgramConfig,
    signer: crate::config::SignerConfig,
    market_row: &crate::config::MarketConfig,
    action_side: &str,
) -> ResolvedBuildAndPostContext {
    use crate::config::resolve_quote_asset_for_offer;
    use crate::offer::ResolvedMarketOfferAssets;
    use serde_json::json;

    let mut market_row = market_row.clone();
    if resolve_quote_price_for_pricing(&market_row.pricing).is_err() {
        if let Some(pricing) = market_row.pricing.as_object_mut() {
            pricing.insert("fixed_quote_per_base".to_string(), json!(1.0));
        } else {
            market_row.pricing = json!({"fixed_quote_per_base": 1.0});
        }
    }
    let quote_asset_for_offer =
        resolve_quote_asset_for_offer(market_row.quote_asset.trim(), "mainnet");
    let mut ctx = sample_resolved_build_and_post_context();
    ctx.gated.program = program;
    ctx.gated.signer = signer;
    ctx.gated.market_row = market_row.clone();
    ctx.action_side_override = Some(action_side.to_string());
    ctx.offer_assets = ResolvedMarketOfferAssets {
        base_asset_id: market_row.base_asset.trim().to_string(),
        quote_asset_id: quote_asset_for_offer.clone(),
        quote_asset_for_offer,
    };
    ctx
}

#[cfg(test)]
pub(crate) fn sample_resolved_build_and_post_context() -> ResolvedBuildAndPostContext {
    use std::collections::HashMap;

    use chia_protocol::Bytes32;
    use serde_json::json;

    use crate::config::{empty_cat_ticker_index, ManagerProgramConfig, MarketConfig};
    use crate::vault::context::VaultCustodySnapshot;

    ResolvedBuildAndPostContext {
        gated: GatedOperatorMarket {
            program: ManagerProgramConfig {
                tx_block_websocket_reconnect_interval_seconds: 1,
                tx_block_fallback_poll_interval_seconds: 1,
                ..Default::default()
            },
            signer: crate::config::SignerConfig {
                network: "mainnet".to_string(),
                coinset_base_url: String::new(),
                kms_key_id: String::new(),
                kms_region: String::new(),
                kms_public_key_hex: None,
                kms_runtime: crate::kms::KmsRuntime::default(),
                vault: VaultCustodySnapshot {
                    launcher_id: Bytes32::default(),
                    custody_threshold: 1,
                    recovery_threshold: 1,
                    recovery_clawback_timelock: 0,
                    custody_keys: Vec::new(),
                    recovery_keys: Vec::new(),
                },
            },
            market_row: MarketConfig {
                market_id: "m1".to_string(),
                enabled: true,
                base_asset: "a1".to_string(),
                base_symbol: "A1".to_string(),
                quote_asset: "xch".to_string(),
                quote_asset_type: "unstable".to_string(),
                receive_address: "xch1".to_string(),
                signer_key_id: "key-main-1".to_string(),
                mode: "sell_only".to_string(),
                pricing: json!({"fixed_quote_per_base": 1.0}),
                cancel_move_threshold_bps: None,
                ladders: HashMap::default(),
            },
            ticker_index: empty_cat_ticker_index(),
            operator_network: "mainnet".to_string(),
        },
        publish_venue: "dexie".to_string(),
        dexie_base_url: "https://api.dexie.space".to_string(),
        splash_base_url: "http://localhost:4000".to_string(),
        offer_assets: ResolvedMarketOfferAssets {
            base_asset_id: "a1".to_string(),
            quote_asset_id: "xch".to_string(),
            quote_asset_for_offer: "xch".to_string(),
        },
        action_side_override: None,
        offer_fee_mojos: 0,
        offer_fee_source: "coinset_fee_unavailable".to_string(),
        #[cfg(test)]
        test_overrides: BuildOfferTestOverrides::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_maker_offer_fee;
    use crate::test_support::signer_config::test_signer_config;

    #[tokio::test]
    async fn resolve_maker_offer_fee_uses_signer_coinset_endpoint() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_fee_estimate")
            .with_status(200)
            .with_body(r#"{"success":true,"estimates":[100,250]}"#)
            .create_async()
            .await;
        let signer = test_signer_config(&server.url());

        let (fee_mojos, fee_source) = resolve_maker_offer_fee(&signer, "mainnet").await;

        assert_eq!(fee_mojos, 250);
        assert_eq!(fee_source, "coinset_conservative_fee");
    }

    #[tokio::test]
    async fn resolve_maker_offer_fee_reports_unavailable_on_lookup_failure() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_fee_estimate")
            .with_status(500)
            .create_async()
            .await;
        let signer = test_signer_config(&server.url());

        let (fee_mojos, fee_source) = resolve_maker_offer_fee(&signer, "mainnet").await;

        assert_eq!(fee_mojos, 0);
        assert_eq!(fee_source, "coinset_fee_unavailable");
    }

    #[tokio::test]
    async fn resolve_build_and_post_offer_assets_normalize_xch_on_testnet11() {
        use std::collections::HashMap;

        use serde_json::json;

        use crate::config::MarketConfig;
        use crate::offer::OfferAssetResolver;
        use crate::test_support::signer_config::test_signer_config;

        let cat = "a".repeat(64);
        let market = MarketConfig {
            market_id: "m1".to_string(),
            enabled: true,
            base_asset: cat.clone(),
            base_symbol: "A1".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "unstable".to_string(),
            receive_address: "xch1".to_string(),
            signer_key_id: "key-main-1".to_string(),
            mode: "sell_only".to_string(),
            pricing: json!({}),
            cancel_move_threshold_bps: None,
            ladders: HashMap::default(),
        };
        let signer = test_signer_config("http://coinset.test");
        let empty_index = crate::config::empty_cat_ticker_index();
        let resolver = OfferAssetResolver::new(&signer, &empty_index, "testnet11");

        let assets = resolver
            .resolve_market_assets(&market)
            .await
            .expect("resolve offer assets");

        assert_eq!(market.quote_asset, "xch");
        assert_eq!(assets.quote_asset_for_offer, "txch");
        assert_eq!(assets.quote_asset_id, "txch");
        assert_ne!(assets.quote_asset_for_offer, market.quote_asset);
    }
}

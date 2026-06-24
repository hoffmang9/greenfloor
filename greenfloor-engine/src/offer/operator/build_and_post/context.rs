use crate::coinset::get_conservative_fee_estimate_for_signer;
use crate::config::{
    action_side_from_pricing, load_gated_operator_market, resolve_offer_publish_settings,
    GatedOperatorMarket, OperatorMarketCommand,
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
    pub quote_price: f64,
    pub action_side: String,
    pub offer_fee_mojos: u64,
    pub offer_fee_source: String,
    #[cfg(test)]
    pub test_overrides: BuildOfferTestOverrides,
}

pub(super) async fn resolve_build_and_post_context(
    request: &BuildAndPostOfferRequest,
) -> SignerResult<ResolvedBuildAndPostContext> {
    let gated = load_gated_operator_market(
        &request.program_path,
        &request.markets_path,
        request.testnet_markets_path.as_deref(),
        None,
        &request.network,
        request.market_id.as_deref(),
        request.pair.as_deref(),
        OperatorMarketCommand::Build,
    )?;
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
    let assets = resolver.resolve_market_assets(&gated.market).await?;
    let quote_price = resolve_quote_price_for_pricing(&gated.market.pricing)?;
    let action_side = resolve_action_side(request.action_side.as_deref(), &gated.market.pricing);
    let (offer_fee_mojos, offer_fee_source) =
        resolve_maker_offer_fee(&gated.signer, &gated.operator_network).await;

    Ok(ResolvedBuildAndPostContext {
        gated,
        publish_venue,
        dexie_base_url,
        splash_base_url,
        offer_assets: assets,
        quote_price,
        action_side,
        offer_fee_mojos,
        offer_fee_source,
        #[cfg(test)]
        test_overrides: request.test_overrides.clone(),
    })
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
            market: MarketConfig {
                market_id: "m1".to_string(),
                enabled: true,
                base_asset: "a1".to_string(),
                base_symbol: "A1".to_string(),
                quote_asset: "xch".to_string(),
                quote_asset_type: "unstable".to_string(),
                receive_address: "xch1".to_string(),
                signer_key_id: "key-main-1".to_string(),
                mode: "sell_only".to_string(),
                pricing: json!({}),
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
        quote_price: 1.0,
        action_side: "sell".to_string(),
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
        let signer = test_signer_config("http://127.0.0.1:1");
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

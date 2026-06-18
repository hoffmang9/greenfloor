use crate::coinset::get_conservative_fee_estimate;
use crate::config::{
    action_side_from_pricing, load_markets_config_with_overlay, load_program_bundle_gated,
    resolve_market_for_build, resolve_offer_publish_settings, ManagerProgramConfig, MarketConfig,
    SignerConfig,
};
use crate::error::SignerResult;
use crate::offer::build_context::resolve_quote_price_for_pricing;
use crate::offer::{normalize_offer_side, resolve_offer_assets_for_action};

use super::BuildAndPostOfferRequest;
use crate::offer::operator::logging::{
    initialize_manager_file_logging, warn_if_log_level_auto_healed,
};
use crate::offer::operator::test_overrides::OfferOperatorTestOverrides;

#[derive(Debug, Clone)]
pub(crate) struct ResolvedBuildAndPostContext {
    pub program: ManagerProgramConfig,
    pub market: MarketConfig,
    pub signer_config: SignerConfig,
    pub publish_venue: String,
    pub dexie_base_url: String,
    pub splash_base_url: String,
    pub resolved_base_asset_id: String,
    pub resolved_quote_asset_id: String,
    pub quote_price: f64,
    pub action_side: String,
    pub offer_fee_mojos: u64,
    pub offer_fee_source: String,
    pub test_overrides: OfferOperatorTestOverrides,
}

pub(super) async fn resolve_build_and_post_context(
    request: &BuildAndPostOfferRequest,
) -> SignerResult<ResolvedBuildAndPostContext> {
    let bundle = load_program_bundle_gated(&request.program_path)?;
    let program = bundle.program;
    initialize_manager_file_logging(&program.home_dir, &program.app_log_level)?;
    warn_if_log_level_auto_healed(program.app_log_level_was_missing, &request.program_path);
    let markets = load_markets_config_with_overlay(
        &request.markets_path,
        request.testnet_markets_path.as_deref(),
    )?;
    let market = resolve_market_for_build(
        &markets,
        request.market_id.as_deref(),
        request.pair.as_deref(),
        &request.network,
    )?;
    let (publish_venue, dexie_base_url, splash_base_url) = resolve_offer_publish_settings(
        &program,
        &request.network,
        request.publish_venue.as_deref(),
        request.dexie_base_url.as_deref(),
        request.splash_base_url.as_deref(),
    )?;
    let signer_config = bundle.signer;
    let (resolved_base_asset_id, resolved_quote_asset_id) =
        resolve_offer_assets_for_action(&signer_config, &market.base_asset, &market.quote_asset)
            .await?;
    let quote_price = resolve_quote_price_for_pricing(&market.pricing)?;
    let action_side = resolve_action_side(request.action_side.as_deref(), &market.pricing);
    let (offer_fee_mojos, offer_fee_source) = resolve_maker_offer_fee(&request.network).await;

    Ok(ResolvedBuildAndPostContext {
        program,
        market,
        signer_config,
        publish_venue,
        dexie_base_url,
        splash_base_url,
        resolved_base_asset_id,
        resolved_quote_asset_id,
        quote_price,
        action_side,
        offer_fee_mojos,
        offer_fee_source,
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

async fn resolve_maker_offer_fee(network: &str) -> (u64, String) {
    match get_conservative_fee_estimate(network, None, 1_000_000, Some(1)).await {
        Ok(Some(fee)) => (fee, "coinset_conservative_fee".to_string()),
        Ok(None) | Err(_) => (0, "coinset_fee_unavailable".to_string()),
    }
}

#[cfg(test)]
pub(crate) fn sample_resolved_build_and_post_context() -> ResolvedBuildAndPostContext {
    use std::collections::HashMap;

    use chia_protocol::Bytes32;
    use serde_json::json;

    use crate::vault::context::VaultCustodySnapshot;

    ResolvedBuildAndPostContext {
        program: ManagerProgramConfig {
            tx_block_websocket_reconnect_interval_seconds: 1,
            tx_block_fallback_poll_interval_seconds: 1,
            ..Default::default()
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
        signer_config: SignerConfig {
            network: "mainnet".to_string(),
            coinset_msp_base_url: String::new(),
            kms_key_id: String::new(),
            kms_region: String::new(),
            kms_public_key_hex: None,
            vault: VaultCustodySnapshot {
                launcher_id: Bytes32::default(),
                custody_threshold: 1,
                recovery_threshold: 1,
                recovery_clawback_timelock: 0,
                custody_keys: Vec::new(),
                recovery_keys: Vec::new(),
            },
        },
        publish_venue: "dexie".to_string(),
        dexie_base_url: "https://api.dexie.space".to_string(),
        splash_base_url: "http://localhost:4000".to_string(),
        resolved_base_asset_id: "a1".to_string(),
        resolved_quote_asset_id: "xch".to_string(),
        quote_price: 1.0,
        action_side: "sell".to_string(),
        offer_fee_mojos: 0,
        offer_fee_source: "coinset_fee_unavailable".to_string(),
        test_overrides: OfferOperatorTestOverrides::default(),
    }
}

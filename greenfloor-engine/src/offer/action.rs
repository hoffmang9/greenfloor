//! Unified offer build for market actions (signer vault KMS path).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::coinset::parse_coin_ids;
use crate::config::{CatTickerIndex, MarketConfig, SignerConfig};
use crate::error::{SignerError, SignerResult};
use crate::offer::assets::{OfferAssetResolver, ResolvedMarketOfferAssets};
use crate::offer::build::build_vault_cat_offer;
use crate::offer::build_context::{
    resolve_offer_expiry_for_pricing, resolve_quote_price_for_pricing,
};
use crate::offer::request::{compute_signer_offer_leg_amounts, normalize_offer_side};
use crate::offer::types::{CreateOfferRequest, CreateOfferResult};

#[derive(Debug, Clone, Deserialize)]
pub struct BuildOfferForActionRequest {
    pub receive_address: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub size_base_units: u64,
    pub action_side: String,
    pub pricing: Value,
    #[serde(default)]
    pub quote_price: Option<f64>,
    #[serde(default = "default_true")]
    pub split_input_coins: bool,
    #[serde(default = "default_true")]
    pub broadcast_split: bool,
    #[serde(default)]
    pub offer_coin_ids: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildOfferForActionResult {
    pub offer_text: String,
    pub side: String,
    pub expires_at_unix: u64,
    pub offer_amount: u64,
    pub request_amount: u64,
    pub execution_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_result: Option<CreateOfferResult>,
}

/// Expires at unix from pricing.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn expires_at_unix_from_pricing(pricing: &Value) -> SignerResult<u64> {
    let (_unit, minutes) = resolve_offer_expiry_for_pricing(pricing);
    let secs = minutes.saturating_mul(60);
    let secs_u64 =
        crate::config::parse_non_negative_u64(secs, "pricing.strategy_offer_expiry_seconds")?;
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().saturating_add(secs_u64))
        .map_err(|_| SignerError::Other("system clock before unix epoch".to_string()))
}

fn resolve_quote_price(request: &BuildOfferForActionRequest) -> SignerResult<f64> {
    if let Some(price) = request.quote_price {
        if price > 0.0 {
            return Ok(price);
        }
    }
    resolve_quote_price_for_pricing(&request.pricing)
}

/// Resolve a market base asset id for coin-op and inventory paths (xch quote leg).
///
/// # Errors
///
/// Returns an error if asset resolution fails.
pub async fn resolve_market_base_asset_id(
    resolver: &OfferAssetResolver<'_>,
    base_asset: &str,
) -> SignerResult<String> {
    resolver.resolve_base(base_asset).await
}

/// Resolve base and quote asset ids for a configured market row (offer build / reservations).
///
/// # Errors
///
/// Returns an error if asset resolution fails.
pub async fn resolve_market_offer_assets_for_action(
    resolver: &OfferAssetResolver<'_>,
    market: &MarketConfig,
    program_network: &str,
) -> SignerResult<ResolvedMarketOfferAssets> {
    resolver
        .resolve_market_assets(market, program_network)
        .await
}

/// Resolve fee-leg asset id for parallel offer reservations.
///
/// # Errors
///
/// Returns an error if asset resolution fails.
pub async fn resolve_market_offer_fee_asset_id(
    resolver: &OfferAssetResolver<'_>,
    assets: &ResolvedMarketOfferAssets,
) -> SignerResult<String> {
    resolver.resolve_fee_asset(assets).await
}

fn leg_amounts_for_request(
    request: &BuildOfferForActionRequest,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    quote_price: f64,
) -> SignerResult<crate::offer::request::SignerOfferLegAmounts> {
    let size =
        i64::try_from(request.size_base_units).map_err(|_| SignerError::InvalidSizeBaseUnits)?;
    compute_signer_offer_leg_amounts(
        size,
        quote_price,
        resolved_base_asset_id,
        resolved_quote_asset_id,
        &request.action_side,
        &request.pricing,
    )
}

fn create_offer_request_from_leg(
    request: &BuildOfferForActionRequest,
    leg: &crate::offer::request::SignerOfferLegAmounts,
    expires_at_unix: u64,
) -> SignerResult<CreateOfferRequest> {
    Ok(CreateOfferRequest {
        receive_address: request.receive_address.clone(),
        offer_asset_id: leg.offer_asset_id.clone(),
        offer_amount: leg.offer_amount_mojos,
        request_asset_id: leg.request_asset_id.clone(),
        request_amount: leg.request_amount_mojos,
        offer_coin_ids: parse_coin_ids(&request.offer_coin_ids)?,
        presplit_coin_ids: Vec::new(),
        split_input_coins: request.split_input_coins,
        broadcast_split: request.broadcast_split,
        expires_at: Some(expires_at_unix),
    })
}

/// Build signer offer for action.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn build_signer_offer_for_action(
    config: SignerConfig,
    request: BuildOfferForActionRequest,
    ticker_index: &CatTickerIndex,
) -> SignerResult<BuildOfferForActionResult> {
    let resolver = OfferAssetResolver::new(&config, ticker_index);
    let (resolved_base, resolved_quote) = resolver
        .resolve_pair(&request.base_asset, &request.quote_asset)
        .await?;
    let quote_price = resolve_quote_price(&request)?;
    let leg = leg_amounts_for_request(&request, &resolved_base, &resolved_quote, quote_price)?;
    let expires_at_unix = expires_at_unix_from_pricing(&request.pricing)?;
    let side = normalize_offer_side(&request.action_side).to_string();
    let create_request = create_offer_request_from_leg(&request, &leg, expires_at_unix)?;
    let create_result = build_vault_cat_offer(config, create_request).await?;

    Ok(BuildOfferForActionResult {
        offer_text: create_result.offer.clone(),
        side,
        expires_at_unix,
        offer_amount: leg.offer_amount_mojos,
        request_amount: leg.request_amount_mojos,
        execution_mode: create_result.execution_mode.to_string(),
        create_result: Some(create_result),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expires_at_from_minutes_pricing() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        let expires = expires_at_unix_from_pricing(&json!({"strategy_offer_expiry_minutes": 5}))
            .expect("expiry");
        assert!(expires >= before + 300);
        assert!(expires <= before + 301);
    }
}

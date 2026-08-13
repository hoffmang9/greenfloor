//! Unified offer build for market actions (signer vault KMS path).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::coinset::parse_coin_ids;
use crate::config::{
    bake_expiry_into_conditions_for_quote, CatTickerIndex, MarketConfig, MarketPricing,
    SignerConfig,
};
use crate::error::{OfferError, SignerError, SignerResult};
use crate::offer::assets::OfferAssetResolver;
use crate::offer::build::build_vault_cat_offer;
use crate::offer::build_context::resolve_offer_expiry_for_pricing;
use crate::offer::request::{compute_signer_offer_leg_amounts, normalize_offer_side};
use crate::offer::types::{
    effective_maker_reuse, CreateOfferRequest, CreateOfferResult, OfferTerms, PresplitMakerReuse,
};

#[derive(Debug, Clone, Deserialize)]
pub struct BuildOfferForActionRequest {
    pub receive_address: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub size_base_units: u64,
    pub action_side: String,
    pub pricing: MarketPricing,
    #[serde(default)]
    pub quote_price: Option<f64>,
    #[serde(default = "default_true")]
    pub split_input_coins: bool,
    #[serde(default = "default_true")]
    pub broadcast_split: bool,
    #[serde(default)]
    pub offer_coin_ids: Vec<String>,
    /// Market `quote_asset_type`; stable omits on-chain expiry from presplit CONDITIONS.
    #[serde(default)]
    pub quote_asset_type: String,
    /// When set, builds `PresplitExisting` against this maker (no new split).
    #[serde(default)]
    pub maker_reuse: Option<PresplitMakerReuse>,
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
    pub execution_mode: crate::offer::OfferExecutionMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_result: Option<CreateOfferResult>,
}

/// Expires at unix from pricing.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn expires_at_unix_from_pricing(pricing: &MarketPricing) -> SignerResult<u64> {
    let (_unit, minutes) = resolve_offer_expiry_for_pricing(pricing);
    let secs = minutes.saturating_mul(60);
    let secs_u64 =
        crate::config::parse_non_negative_u64(secs, "pricing.strategy_offer_expiry_seconds")?;
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().saturating_add(secs_u64))
        .map_err(|_| SignerError::Other("system clock before unix epoch".to_string()))
}

/// Build offer terms from already-resolved market assets (same ids create/post use).
///
/// # Errors
///
/// Returns an error when pricing or size conversion fails.
pub(crate) fn offer_terms_from_resolved_assets(
    market: &MarketConfig,
    assets: &crate::offer::assets::ResolvedMarketOfferAssets,
    size_base_units: u64,
    side: &str,
) -> SignerResult<OfferTerms> {
    let quote_price = market.quote_price_for_side(side)?;
    let size_i64 = i64::try_from(size_base_units)
        .map_err(|_| SignerError::Offer(OfferError::InvalidSizeBaseUnits))?;
    let leg = compute_signer_offer_leg_amounts(
        size_i64,
        quote_price,
        &assets.base_asset_id,
        &assets.quote_asset_id,
        side,
        &market.pricing,
    )?;
    let expires_at = expires_at_unix_from_pricing(&market.pricing)?;
    Ok(OfferTerms {
        receive_address: market.receive_address.clone(),
        offer_asset_id: leg.offer_asset_id,
        offer_amount: leg.offer_amount_mojos,
        request_asset_id: leg.request_asset_id,
        request_amount: leg.request_amount_mojos,
        expires_at: Some(expires_at),
        bake_expiry_into_conditions: bake_expiry_into_conditions_for_quote(
            &market.quote_asset_type,
        ),
    })
}

/// Plan offer terms for a market size/side (shared by ensure hash compare and create).
///
/// Uses [`OfferAssetResolver::resolve_market_assets`] so quote normalization matches post.
///
/// # Errors
///
/// Returns an error when asset resolve, pricing, or size conversion fails.
pub(crate) async fn plan_offer_terms_for_market(
    signer: &SignerConfig,
    ticker_index: &CatTickerIndex,
    network: &str,
    market: &MarketConfig,
    size_base_units: u64,
    side: &str,
) -> SignerResult<OfferTerms> {
    let resolver = OfferAssetResolver::new(signer, ticker_index, network);
    let assets = resolver.resolve_market_assets(market).await?;
    offer_terms_from_resolved_assets(market, &assets, size_base_units, side)
}

fn resolve_quote_price(request: &BuildOfferForActionRequest) -> SignerResult<f64> {
    if let Some(price) = request.quote_price.filter(|price| *price > 0.0) {
        return Ok(price);
    }
    request.pricing.quote_price()
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
    operator_network: &str,
) -> SignerResult<BuildOfferForActionResult> {
    let resolver = OfferAssetResolver::new(&config, ticker_index, operator_network);
    let (resolved_base, resolved_quote) = resolver
        .resolve_pair(&request.base_asset, &request.quote_asset)
        .await?;
    let quote_price = resolve_quote_price(&request)?;
    let leg = leg_amounts_for_request(&request, &resolved_base, &resolved_quote, quote_price)?;
    let expires_at_unix = expires_at_unix_from_pricing(&request.pricing)?;
    let side = normalize_offer_side(&request.action_side).to_string();
    let create_request = create_offer_request_from_leg(&request, &leg, expires_at_unix)?;
    let create_result =
        build_vault_cat_offer(config, operator_network.to_string(), create_request).await?;

    Ok(BuildOfferForActionResult {
        offer_text: create_result.offer.clone(),
        side,
        expires_at_unix,
        offer_amount: leg.offer_amount_mojos,
        request_amount: leg.request_amount_mojos,
        execution_mode: create_result.execution_mode,
        create_result: Some(create_result),
    })
}

fn leg_amounts_for_request(
    request: &BuildOfferForActionRequest,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    quote_price: f64,
) -> SignerResult<crate::offer::request::SignerOfferLegAmounts> {
    let size = i64::try_from(request.size_base_units)
        .map_err(|_| SignerError::Offer(OfferError::InvalidSizeBaseUnits))?;
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
    let reuse = effective_maker_reuse(request.maker_reuse.as_ref());
    let presplit_coin_ids = if let Some(reuse) = reuse {
        parse_coin_ids(std::slice::from_ref(&reuse.coin_id))?
    } else {
        Vec::new()
    };
    let offer_nonce = match reuse {
        Some(reuse) => Some(crate::hex::hex_to_bytes32(&reuse.offer_nonce)?),
        None => None,
    };
    Ok(CreateOfferRequest {
        receive_address: request.receive_address.clone(),
        offer_asset_id: leg.offer_asset_id.clone(),
        offer_amount: leg.offer_amount_mojos,
        request_asset_id: leg.request_asset_id.clone(),
        request_amount: leg.request_amount_mojos,
        offer_coin_ids: parse_coin_ids(&request.offer_coin_ids)?,
        presplit_coin_ids,
        split_input_coins: request.split_input_coins && reuse.is_none(),
        broadcast_split: request.broadcast_split && reuse.is_none(),
        expires_at: Some(expires_at_unix),
        bake_expiry_into_conditions: bake_expiry_into_conditions_for_quote(
            &request.quote_asset_type,
        ),
        offer_nonce,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MarketPricing;
    use crate::offer::request::SignerOfferLegAmounts;

    #[test]
    fn expires_at_from_minutes_pricing() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        let pricing = MarketPricing {
            strategy_offer_expiry_minutes: Some(5),
            ..MarketPricing::default()
        };
        let expires = expires_at_unix_from_pricing(&pricing).expect("expiry");
        assert!(expires >= before + 300);
        assert!(expires <= before + 301);
    }

    fn sample_action_request(reuse: Option<PresplitMakerReuse>) -> BuildOfferForActionRequest {
        BuildOfferForActionRequest {
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w"
                .to_string(),
            base_asset: "aa".repeat(32),
            quote_asset: "xch".to_string(),
            size_base_units: 1_000,
            action_side: "sell".to_string(),
            pricing: MarketPricing::default(),
            quote_price: Some(1.0),
            split_input_coins: true,
            broadcast_split: true,
            offer_coin_ids: Vec::new(),
            quote_asset_type: "stable".to_string(),
            maker_reuse: reuse,
        }
    }

    #[test]
    fn create_offer_request_from_leg_omits_split_when_reusing_maker() {
        let request = sample_action_request(Some(PresplitMakerReuse {
            coin_id: "ab".repeat(32),
            offer_nonce: "cd".repeat(32),
        }));
        let leg = SignerOfferLegAmounts {
            offer_asset_id: "ab".repeat(32),
            offer_amount_mojos: 1_000,
            request_asset_id: "xch".to_string(),
            request_amount_mojos: 1,
        };
        let created = create_offer_request_from_leg(&request, &leg, 1_700_000_000).expect("create");
        assert!(!created.split_input_coins);
        assert!(!created.broadcast_split);
        assert!(!created.bake_expiry_into_conditions);
        assert_eq!(created.presplit_coin_ids.len(), 1);
        assert!(created.offer_nonce.is_some());
    }

    #[test]
    fn create_offer_request_from_leg_presplit_new_without_reuse() {
        let request = sample_action_request(None);
        let leg = SignerOfferLegAmounts {
            offer_asset_id: "ab".repeat(32),
            offer_amount_mojos: 1_000,
            request_asset_id: "xch".to_string(),
            request_amount_mojos: 1,
        };
        let created = create_offer_request_from_leg(&request, &leg, 1_700_000_000).expect("create");
        assert!(created.split_input_coins);
        assert!(created.broadcast_split);
        assert!(created.presplit_coin_ids.is_empty());
        assert!(created.offer_nonce.is_none());
    }

    #[test]
    fn offer_terms_from_resolved_assets_uses_normalized_quote_ids() {
        use crate::offer::assets::ResolvedMarketOfferAssets;
        use crate::test_support::market_config::sample_market;

        let mut market =
            sample_market("xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w");
        market.quote_asset = "xch".into();
        market.quote_asset_type = "stable".into();
        market.pricing = MarketPricing {
            fixed_quote_per_base: Some(1.0),
            ..MarketPricing::default()
        };
        let assets = ResolvedMarketOfferAssets {
            base_asset_id: "aa".repeat(32),
            quote_asset_id: "xch".into(),
            quote_asset_for_offer: "txch".into(),
        };
        let terms =
            offer_terms_from_resolved_assets(&market, &assets, 1_000, "sell").expect("terms");
        assert_eq!(terms.offer_asset_id, "aa".repeat(32));
        assert_eq!(terms.request_asset_id, "xch");
        assert!(!terms.bake_expiry_into_conditions);
    }

    #[test]
    fn two_sided_spread_puts_par_inside_bid_and_ask() {
        use crate::offer::assets::ResolvedMarketOfferAssets;
        use crate::test_support::market_config::sample_market;

        let quote = "bb".repeat(32);
        let mut market =
            sample_market("xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w");
        market.mode = "two_sided".into();
        market.quote_asset_type = "stable".into();
        market.pricing = MarketPricing {
            fixed_quote_per_base: Some(1.0),
            strategy_target_spread_bps: Some(20),
            base_unit_mojo_multiplier: Some(1_000),
            quote_unit_mojo_multiplier: Some(1_000),
            ..MarketPricing::default()
        };
        let assets = ResolvedMarketOfferAssets {
            base_asset_id: "aa".repeat(32),
            quote_asset_id: quote.clone(),
            quote_asset_for_offer: quote.clone(),
        };
        let sell = offer_terms_from_resolved_assets(&market, &assets, 10, "sell").expect("sell");
        let buy = offer_terms_from_resolved_assets(&market, &assets, 10, "buy").expect("buy");
        assert_eq!(sell.offer_amount, 10_000);
        assert_eq!(sell.request_amount, 10_010);
        assert_eq!(buy.offer_amount, 9_990);
        assert_eq!(buy.request_amount, 10_000);
    }
}

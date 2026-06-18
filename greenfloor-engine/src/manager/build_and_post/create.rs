use crate::config::{MarketConfig, SignerConfig};
use crate::error::SignerResult;
use crate::offer::action::BuildOfferForActionResult;
use crate::offer::{build_signer_offer_for_action, BuildOfferForActionRequest};

pub(super) async fn create_offer(
    signer_config: &SignerConfig,
    market: &MarketConfig,
    size_base_units: u64,
    quote_price: f64,
    action_side: &str,
) -> SignerResult<BuildOfferForActionResult> {
    #[cfg(debug_assertions)]
    if let Ok(offer_text) = std::env::var("GREENFLOOR_TEST_OFFER_TEXT") {
        let offer_text = offer_text.trim().to_string();
        if !offer_text.is_empty() {
            return Ok(BuildOfferForActionResult {
                offer_text,
                side: action_side.to_string(),
                expires_at_unix: 4_000_000_000,
                offer_amount: size_base_units,
                request_amount: 1,
                execution_mode: "signer_test_stub".to_string(),
                create_result: None,
            });
        }
    }
    let request = BuildOfferForActionRequest {
        receive_address: market.receive_address.clone(),
        base_asset: market.base_asset.clone(),
        quote_asset: market.quote_asset.clone(),
        size_base_units,
        action_side: action_side.to_string(),
        pricing: market.pricing.clone(),
        quote_price: Some(quote_price),
        split_input_coins: true,
        broadcast_split: true,
        offer_coin_ids: Vec::new(),
    };
    build_signer_offer_for_action(signer_config.clone(), request).await
}

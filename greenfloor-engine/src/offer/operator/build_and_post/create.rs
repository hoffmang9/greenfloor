use crate::error::SignerResult;
use crate::offer::action::BuildOfferForActionResult;
use crate::offer::{build_signer_offer_for_action, BuildOfferForActionRequest};

use super::context::ResolvedBuildAndPostContext;

pub(super) async fn create_offer(
    ctx: &ResolvedBuildAndPostContext,
    size_base_units: u64,
) -> SignerResult<BuildOfferForActionResult> {
    #[cfg(test)]
    if let Some(offer_text) = ctx.test_overrides.stub_offer_text() {
        return Ok(BuildOfferForActionResult {
            offer_text: offer_text.to_string(),
            side: ctx.action_side.clone(),
            expires_at_unix: 4_000_000_000,
            offer_amount: size_base_units,
            request_amount: 1,
            execution_mode: "signer_test_stub".to_string(),
            create_result: None,
        });
    }
    let request = BuildOfferForActionRequest {
        receive_address: ctx.market.receive_address.clone(),
        base_asset: ctx.market.base_asset.clone(),
        quote_asset: ctx.market.quote_asset.clone(),
        size_base_units,
        action_side: ctx.action_side.clone(),
        pricing: ctx.market.pricing.clone(),
        quote_price: Some(ctx.quote_price),
        // Presplit (ent-wallet `splitInputCoins`): vault singleton spends only in the split tx;
        // the Dexie offer file is self-contained so one taker fill does not invalidate siblings.
        split_input_coins: true,
        broadcast_split: true,
        offer_coin_ids: Vec::new(),
    };
    build_signer_offer_for_action(ctx.signer_config.clone(), request).await
}

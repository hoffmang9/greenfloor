use crate::error::SignerResult;
use crate::offer::action::BuildOfferForActionResult;
use crate::offer::types::PresplitMakerReuse;
use crate::offer::{build_signer_offer_for_action, BuildOfferForActionRequest};

use super::context::ResolvedBuildAndPostContext;

pub(super) fn build_create_offer_request(
    ctx: &ResolvedBuildAndPostContext,
    size_base_units: u64,
    maker_reuse: Option<PresplitMakerReuse>,
    offer_coin_ids: Vec<String>,
) -> SignerResult<BuildOfferForActionRequest> {
    let reuse = maker_reuse.is_some();
    Ok(BuildOfferForActionRequest {
        receive_address: ctx.gated.market_row.receive_address.clone(),
        base_asset: ctx.gated.market_row.base_asset.clone(),
        quote_asset: ctx.offer_assets.quote_asset_for_offer.clone(),
        size_base_units,
        action_side: ctx.action_side(),
        pricing: ctx.gated.market_row.pricing.clone(),
        quote_price: Some(ctx.quote_price()?),
        // Presplit (ent-wallet `splitInputCoins`): vault singleton spends only in the split tx;
        // the Dexie offer file is self-contained so one taker fill does not invalidate siblings.
        // Maker reuse skips split (coin already exists).
        split_input_coins: !reuse,
        broadcast_split: !reuse,
        offer_coin_ids,
        quote_asset_type: ctx.gated.market_row.quote_asset_type.clone(),
        maker_reuse,
    })
}

pub(super) async fn create_offer(
    ctx: &ResolvedBuildAndPostContext,
    size_base_units: u64,
    maker_reuse: Option<PresplitMakerReuse>,
    offer_coin_ids: Vec<String>,
) -> SignerResult<BuildOfferForActionResult> {
    #[cfg(test)]
    if let Some(offer_text) = ctx.test_overrides.stub_offer_text() {
        return Ok(BuildOfferForActionResult {
            offer_text: offer_text.to_string(),
            side: ctx.action_side(),
            expires_at_unix: 4_000_000_000,
            offer_amount: size_base_units,
            request_amount: 1,
            execution_mode: "signer_test_stub".to_string(),
            create_result: None,
        });
    }
    let request = build_create_offer_request(ctx, size_base_units, maker_reuse, offer_coin_ids)?;
    build_signer_offer_for_action(
        ctx.gated.signer.clone(),
        request,
        &ctx.gated.ticker_index,
        &ctx.gated.operator_network,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::operator::build_and_post::context::sample_resolved_build_and_post_context;
    use crate::offer::ResolvedMarketOfferAssets;

    #[test]
    fn create_offer_request_uses_normalized_quote_from_offer_assets() {
        let mut ctx = sample_resolved_build_and_post_context();
        ctx.gated.market_row.quote_asset = "xch".to_string();
        ctx.offer_assets = ResolvedMarketOfferAssets {
            base_asset_id: "a1".to_string(),
            quote_asset_id: "txch".to_string(),
            quote_asset_for_offer: "txch".to_string(),
        };

        let request =
            build_create_offer_request(&ctx, 100, None, Vec::new()).expect("create offer request");

        assert_eq!(request.quote_asset, "txch");
        assert_ne!(request.quote_asset, ctx.gated.market_row.quote_asset);
        assert!(request.offer_coin_ids.is_empty());
    }

    #[test]
    fn create_offer_request_pins_non_empty_offer_coin_ids() {
        let ctx = sample_resolved_build_and_post_context();
        let coin = "aa".repeat(32);
        let request = build_create_offer_request(&ctx, 100, None, vec![coin.clone()])
            .expect("create offer request");
        assert_eq!(request.offer_coin_ids, vec![coin]);
    }
}

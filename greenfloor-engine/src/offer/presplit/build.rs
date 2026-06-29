#[cfg(test)]
use chia_protocol::Coin;
use chia_protocol::{Bytes32, SpendBundle};
use chia_sdk_driver::{Cat, CatSpend, Spend, SpendContext};

use crate::error::{SignerError, SignerResult};
use crate::offer::presplit::binding::PresplitOfferBinding;
use crate::offer::presplit::conditions::build_presplit_conditions_inner_spend;
use crate::offer::presplit::pipeline::{PresplitFixedConditions, PresplitPaymentContext};

fn verify_presplit_fixed_conditions(
    built: &PresplitFixedConditions,
    binding: &PresplitOfferBinding,
) -> SignerResult<()> {
    if built.fixed_conditions_tree_hash != binding.fixed_conditions_tree_hash {
        return Err(SignerError::Driver(
            "presplit fixed conditions hash mismatch".to_string(),
        ));
    }
    Ok(())
}

/// Maker coin spent when assembling a presplit offer input bundle.
pub(crate) enum PresplitMakerInput {
    Cat(Cat),
    /// Presplit XCH maker coin for presplit offer assembly (cancel/reclaim roundtrips today).
    #[cfg(test)]
    Xch(Coin),
}

fn spend_presplit_maker_input(
    ctx: &mut SpendContext,
    input: &PresplitMakerInput,
    inner_spend: Spend,
) -> SignerResult<()> {
    match input {
        PresplitMakerInput::Cat(cat) => {
            Cat::spend_all(ctx, &[CatSpend::new(*cat, inner_spend)]).map_err(SignerError::from)?;
            Ok(())
        }
        #[cfg(test)]
        PresplitMakerInput::Xch(coin) => ctx.spend(*coin, inner_spend).map_err(SignerError::from),
    }
}

pub(crate) fn build_offer_from_presplit_input(
    input: &PresplitMakerInput,
    launcher_id: Bytes32,
    binding: &PresplitOfferBinding,
    payment_ctx: &PresplitPaymentContext<'_>,
) -> SignerResult<(String, String, String)> {
    let mut ctx = SpendContext::new();
    let built =
        payment_ctx.build_fixed_conditions(&mut ctx, binding.offer_amount, binding.expires_at)?;
    verify_presplit_fixed_conditions(&built, binding)?;
    let inner_spend =
        build_presplit_conditions_inner_spend(&mut ctx, built.fixed_spend, launcher_id)?;
    spend_presplit_maker_input(&mut ctx, input, inner_spend)?;
    let input_spend_bundle = SpendBundle::new(ctx.take(), chia_bls::Signature::default());
    let (offer_text, spend_bundle_hex) = payment_ctx.encode_offer(input_spend_bundle)?;
    Ok((
        offer_text,
        spend_bundle_hex,
        hex::encode(payment_ctx.offer_nonce()),
    ))
}

pub(crate) fn build_offer_from_presplit_cat(
    presplit_cat: Cat,
    launcher_id: Bytes32,
    binding: &PresplitOfferBinding,
    payment_ctx: &PresplitPaymentContext<'_>,
) -> SignerResult<(String, String, String)> {
    build_offer_from_presplit_input(
        &PresplitMakerInput::Cat(presplit_cat),
        launcher_id,
        binding,
        payment_ctx,
    )
}

#[cfg(test)]
pub(crate) fn build_offer_from_presplit_xch(
    presplit_coin: Coin,
    launcher_id: Bytes32,
    binding: &PresplitOfferBinding,
    payment_ctx: &PresplitPaymentContext<'_>,
) -> SignerResult<(String, String, String)> {
    build_offer_from_presplit_input(
        &PresplitMakerInput::Xch(presplit_coin),
        launcher_id,
        binding,
        payment_ctx,
    )
}

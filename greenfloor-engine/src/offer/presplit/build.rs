//! Presplit offer assembly pipeline.
//!
//! Payment bundles are built three times per presplit-new/existing offer:
//! 1. **Plan** — derive binding hashes ([`PresplitOfferBinding::plan`])
//! 2. **Input spend** — rebuild, verify hash, spend maker coin into the input bundle
//! 3. **Encode** — fresh [`SpendContext`] for [`Offer::from_input_spend_bundle`]
//!
//! [`PresplitPaymentContext`] owns the shared terms/nonce inputs for all three stages.
//! Stages still rebuild payment nodes because allocator-backed CLVM data cannot move across
//! [`SpendContext::take`].

use chia_protocol::{Bytes32, Coin, SpendBundle};
use chia_sdk_driver::{Cat, CatSpend, Offer, Spend, SpendContext};
use clvm_utils::TreeHash;
use clvmr::Allocator;

use crate::bech32m::encode_offer;
use crate::coinset::spend_bundle_hex;
use crate::error::{SignerError, SignerResult};
use crate::offer::plan::build_offer_payment_bundle;
use crate::offer::presplit::binding::PresplitOfferBinding;
use crate::offer::presplit::conditions::{
    build_fixed_presplit_conditions_spend, build_presplit_conditions_inner_spend,
};
use crate::offer::types::OfferTerms;

struct PresplitFixedConditions {
    fixed_spend: Spend,
    fixed_conditions_tree_hash: TreeHash,
}

/// Shared payment inputs for all presplit offer pipeline stages.
pub(crate) struct PresplitPaymentContext<'a> {
    terms: &'a OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
}

impl<'a> PresplitPaymentContext<'a> {
    pub(crate) fn new(
        terms: &'a OfferTerms,
        receive_puzzle_hash: Bytes32,
        offer_nonce: Bytes32,
    ) -> Self {
        Self {
            terms,
            receive_puzzle_hash,
            offer_nonce,
        }
    }

    fn build_fixed_conditions(
        &self,
        ctx: &mut SpendContext,
        offer_amount: u64,
        expires_at: Option<u64>,
    ) -> SignerResult<PresplitFixedConditions> {
        let payments = build_offer_payment_bundle(
            ctx,
            self.terms,
            self.receive_puzzle_hash,
            self.offer_nonce,
        )?;
        let fixed_spend =
            build_fixed_presplit_conditions_spend(ctx, &payments, offer_amount, expires_at)?;
        Ok(PresplitFixedConditions {
            fixed_conditions_tree_hash: ctx.tree_hash(fixed_spend.puzzle),
            fixed_spend,
        })
    }

    fn encode_offer(&self, input_spend_bundle: SpendBundle) -> SignerResult<(String, String)> {
        let mut offer_ctx = SpendContext::new();
        let offer_payments = build_offer_payment_bundle(
            &mut offer_ctx,
            self.terms,
            self.receive_puzzle_hash,
            self.offer_nonce,
        )?;
        let mut allocator = Allocator::new();
        let offer = Offer::from_input_spend_bundle(
            &mut allocator,
            input_spend_bundle,
            offer_payments.requested_payments,
            offer_payments.requested_asset_info,
        )
        .map_err(SignerError::from)?;
        let offer_spend_bundle = offer
            .to_spend_bundle(&mut offer_ctx)
            .map_err(SignerError::from)?;
        Ok((
            encode_offer(&offer_spend_bundle)?,
            spend_bundle_hex(&offer_spend_bundle)?,
        ))
    }
}

/// Maker coin spent when assembling a presplit offer input bundle.
pub(crate) enum PresplitMakerInput {
    Cat(Cat),
    /// Presplit XCH maker coin; match arm kept for presplit XCH offer assembly tests.
    #[cfg_attr(not(test), allow(dead_code))]
    Xch(Coin),
}

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
        PresplitMakerInput::Xch(coin) => ctx.spend(*coin, inner_spend).map_err(SignerError::from),
    }
}

pub(crate) fn build_offer_from_presplit_input(
    input: &PresplitMakerInput,
    launcher_id: Bytes32,
    binding: &PresplitOfferBinding,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<(String, String, String)> {
    let payment_ctx = PresplitPaymentContext::new(terms, receive_puzzle_hash, offer_nonce);
    let mut ctx = SpendContext::new();
    let built =
        payment_ctx.build_fixed_conditions(&mut ctx, binding.offer_amount, binding.expires_at)?;
    verify_presplit_fixed_conditions(&built, binding)?;
    let inner_spend =
        build_presplit_conditions_inner_spend(&mut ctx, built.fixed_spend, launcher_id)?;
    spend_presplit_maker_input(&mut ctx, input, inner_spend)?;
    let input_spend_bundle = SpendBundle::new(ctx.take(), chia_bls::Signature::default());
    let (offer_text, spend_bundle_hex) = payment_ctx.encode_offer(input_spend_bundle)?;
    Ok((offer_text, spend_bundle_hex, hex::encode(offer_nonce)))
}

pub(crate) fn build_offer_from_presplit_cat(
    presplit_cat: Cat,
    launcher_id: Bytes32,
    binding: &PresplitOfferBinding,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<(String, String, String)> {
    build_offer_from_presplit_input(
        &PresplitMakerInput::Cat(presplit_cat),
        launcher_id,
        binding,
        terms,
        receive_puzzle_hash,
        offer_nonce,
    )
}

#[cfg(test)]
pub(crate) fn build_offer_from_presplit_xch(
    presplit_coin: Coin,
    launcher_id: Bytes32,
    binding: &PresplitOfferBinding,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<(String, String, String)> {
    build_offer_from_presplit_input(
        &PresplitMakerInput::Xch(presplit_coin),
        launcher_id,
        binding,
        terms,
        receive_puzzle_hash,
        offer_nonce,
    )
}

pub(crate) fn plan_presplit_binding(
    launcher_id: Bytes32,
    payment_ctx: &PresplitPaymentContext<'_>,
    offer_amount: u64,
    expires_at: Option<u64>,
) -> SignerResult<PresplitOfferBinding> {
    let mut ctx = SpendContext::new();
    let built = payment_ctx.build_fixed_conditions(&mut ctx, offer_amount, expires_at)?;
    let p2_hashes = crate::vault::members::p2_conditions_or_singleton_puzzle_hash(
        built.fixed_conditions_tree_hash,
        launcher_id,
    )?;
    Ok(PresplitOfferBinding {
        offer_amount,
        expires_at,
        fixed_conditions_tree_hash: built.fixed_conditions_tree_hash,
        p2_puzzle_hash: p2_hashes.puzzle_hash.into(),
    })
}

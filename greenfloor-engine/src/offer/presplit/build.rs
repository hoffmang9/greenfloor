//! Presplit offer assembly pipeline.
//!
//! Payment bundles are built three times per presplit-new/existing offer:
//! 1. **Plan** — derive binding hashes ([`PresplitOfferBinding::plan`])
//! 2. **Input spend** — rebuild, verify hash, spend maker coin into the input bundle
//! 3. **Encode** — fresh [`SpendContext`] for [`Offer::from_input_spend_bundle`]
//!
//! Stages cannot share allocator-backed payment nodes across [`SpendContext::take`].

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
use crate::vault::members::p2_conditions_or_singleton_puzzle_hash;

/// Maker coin spent when assembling a presplit offer input bundle.
pub(crate) enum PresplitMakerInput {
    Cat(Cat),
    #[cfg_attr(not(test), allow(dead_code))]
    Xch(Coin),
}

struct PresplitFixedConditions {
    fixed_spend: Spend,
    fixed_conditions_tree_hash: TreeHash,
}

fn build_presplit_fixed_conditions(
    ctx: &mut SpendContext,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
    offer_amount: u64,
    expires_at: Option<u64>,
) -> SignerResult<PresplitFixedConditions> {
    let payments = build_offer_payment_bundle(ctx, terms, receive_puzzle_hash, offer_nonce)?;
    let fixed_spend =
        build_fixed_presplit_conditions_spend(ctx, &payments, offer_amount, expires_at)?;
    Ok(PresplitFixedConditions {
        fixed_conditions_tree_hash: ctx.tree_hash(fixed_spend.puzzle),
        fixed_spend,
    })
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

#[allow(clippy::needless_pass_by_value)]
fn spend_presplit_maker_input(
    ctx: &mut SpendContext,
    input: PresplitMakerInput,
    inner_spend: Spend,
) -> SignerResult<()> {
    match input {
        PresplitMakerInput::Cat(cat) => {
            Cat::spend_all(ctx, &[CatSpend::new(cat, inner_spend)]).map_err(SignerError::from)?;
            Ok(())
        }
        PresplitMakerInput::Xch(coin) => ctx.spend(coin, inner_spend).map_err(SignerError::from),
    }
}

fn encode_presplit_offer(
    input_spend_bundle: SpendBundle,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<(String, String)> {
    let mut offer_ctx = SpendContext::new();
    let offer_payments =
        build_offer_payment_bundle(&mut offer_ctx, terms, receive_puzzle_hash, offer_nonce)?;
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

impl PresplitOfferBinding {
    pub(crate) fn plan(
        launcher_id: Bytes32,
        terms: &OfferTerms,
        receive_puzzle_hash: Bytes32,
        offer_nonce: Bytes32,
    ) -> SignerResult<Self> {
        let mut ctx = SpendContext::new();
        let built = build_presplit_fixed_conditions(
            &mut ctx,
            terms,
            receive_puzzle_hash,
            offer_nonce,
            terms.offer_amount,
            terms.expires_at,
        )?;
        let p2_hashes =
            p2_conditions_or_singleton_puzzle_hash(built.fixed_conditions_tree_hash, launcher_id)?;
        Ok(Self {
            offer_amount: terms.offer_amount,
            expires_at: terms.expires_at,
            fixed_conditions_tree_hash: built.fixed_conditions_tree_hash,
            p2_puzzle_hash: p2_hashes.puzzle_hash.into(),
        })
    }
}

pub(crate) fn build_offer_from_presplit_input(
    input: PresplitMakerInput,
    launcher_id: Bytes32,
    binding: &PresplitOfferBinding,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<(String, String, String)> {
    let mut ctx = SpendContext::new();
    let built = build_presplit_fixed_conditions(
        &mut ctx,
        terms,
        receive_puzzle_hash,
        offer_nonce,
        binding.offer_amount,
        binding.expires_at,
    )?;
    verify_presplit_fixed_conditions(&built, binding)?;
    let inner_spend =
        build_presplit_conditions_inner_spend(&mut ctx, built.fixed_spend, launcher_id)?;
    spend_presplit_maker_input(&mut ctx, input, inner_spend)?;
    let input_spend_bundle = SpendBundle::new(ctx.take(), chia_bls::Signature::default());
    let (offer_text, spend_bundle_hex) =
        encode_presplit_offer(input_spend_bundle, terms, receive_puzzle_hash, offer_nonce)?;
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
        PresplitMakerInput::Cat(presplit_cat),
        launcher_id,
        binding,
        terms,
        receive_puzzle_hash,
        offer_nonce,
    )
}

//! Shared presplit offer payment pipeline stages.
//!
//! Payment bundles are built three times per presplit-new/existing offer:
//! 1. **Plan** — derive binding hashes ([`PresplitOfferBinding::plan`])
//! 2. **Input spend** — rebuild, verify hash, spend maker coin into the input bundle
//! 3. **Encode** — fresh [`SpendContext`] for [`Offer::from_input_spend_bundle`]
//!
//! [`PresplitPaymentContext`] owns the shared terms/nonce inputs for all three stages.
//! Stages still rebuild payment nodes because allocator-backed CLVM data cannot move across
//! [`SpendContext::take`].

use chia_protocol::{Bytes32, SpendBundle};
use chia_sdk_driver::{Offer, Spend, SpendContext};
use clvm_utils::TreeHash;
use clvmr::Allocator;

use crate::bech32m::encode_offer;
use crate::coinset::spend_bundle_hex;
use crate::error::{SignerError, SignerResult};
use crate::offer::plan::build_offer_payment_bundle;
use crate::offer::presplit::conditions::build_fixed_presplit_conditions_spend;
use crate::offer::types::OfferTerms;

pub(crate) struct PresplitFixedConditions {
    pub(crate) fixed_spend: Spend,
    pub(crate) fixed_conditions_tree_hash: TreeHash,
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

    pub(crate) fn offer_nonce(&self) -> Bytes32 {
        self.offer_nonce
    }

    pub(crate) fn build_fixed_conditions(
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

    pub(crate) fn encode_offer(
        &self,
        input_spend_bundle: SpendBundle,
    ) -> SignerResult<(String, String)> {
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

use chia_protocol::Bytes32;
use chia_puzzle_types::{
    offer::{NotarizedPayment, Payment},
    Memos,
};
use chia_sdk_driver::{AssetInfo, RequestedPayments, SpendContext};
use chia_sdk_types::{conditions::AssertBeforeSecondsAbsolute, Conditions};

use crate::coinset::{OfferCoinsetBackend, SelectedCats};
use crate::error::{SignerError, SignerResult};
use crate::hex::hex_to_bytes32;
use crate::offer::presplit::{offer_nonce_from_cats, offer_nonce_from_coin_ids};
use crate::offer::types::{OfferInput, OfferTerms};

use crate::coinset::is_xch_like_asset;

pub(crate) enum OfferPlan {
    ExistingPresplit {
        offer_nonce: Bytes32,
    },
    RequiresSplitFlag,
    Direct {
        selection: SelectedCats,
        offer_nonce: Bytes32,
    },
    SplitAndOffer {
        selection: SelectedCats,
        offer_nonce: Bytes32,
    },
}

pub(crate) fn validate_offer_input(input: &OfferInput) -> SignerResult<()> {
    let terms = input.terms();
    if terms.offer_amount == 0 || terms.request_amount == 0 {
        return Err(SignerError::InvalidOutputAmount);
    }
    if is_xch_like_asset(&terms.offer_asset_id) {
        return Err(SignerError::Other(
            "vault local offer path supports CAT offer side only".to_string(),
        ));
    }
    Ok(())
}

fn offer_nonce_for_existing_presplit(source_coin_ids: &[Bytes32]) -> SignerResult<Bytes32> {
    if source_coin_ids.is_empty() {
        return Err(SignerError::PresplitOfferRequiresSourceCoinIds);
    }
    Ok(offer_nonce_from_coin_ids(source_coin_ids))
}

pub(crate) async fn plan_vault_cat_offer<C: OfferCoinsetBackend>(
    backend: &C,
    input: &OfferInput,
    offer_asset_id: Bytes32,
) -> SignerResult<OfferPlan> {
    match input {
        OfferInput::PresplitExisting {
            source_coin_ids, ..
        } => Ok(OfferPlan::ExistingPresplit {
            offer_nonce: offer_nonce_for_existing_presplit(source_coin_ids)?,
        }),
        OfferInput::Direct {
            terms,
            offer_coin_ids,
        }
        | OfferInput::PresplitNew {
            terms,
            offer_coin_ids,
            ..
        } => {
            let selection = backend
                .select_cats_for_spend(
                    &terms.receive_address,
                    offer_asset_id,
                    offer_coin_ids,
                    terms.offer_amount,
                )
                .await?;
            let offer_nonce = offer_nonce_from_cats(&selection.selected);

            // Exact-size inputs need no vault split; Direct cancel metadata assumes one maker coin.
            if selection.offered_total <= terms.offer_amount {
                if selection.selected.len() != 1 {
                    return Err(SignerError::DirectOfferRequiresSingleInputCoin);
                }
                return Ok(OfferPlan::Direct {
                    selection,
                    offer_nonce,
                });
            }

            Ok(match input {
                OfferInput::PresplitNew { .. } => OfferPlan::SplitAndOffer {
                    selection,
                    offer_nonce,
                },
                OfferInput::Direct { .. } => OfferPlan::RequiresSplitFlag,
                OfferInput::PresplitExisting { .. } => unreachable!(),
            })
        }
    }
}
pub(crate) struct OfferPaymentBundle {
    pub requested_payments: RequestedPayments,
    pub requested_asset_info: AssetInfo,
}

pub(crate) fn build_offer_payment_bundle(
    ctx: &mut SpendContext,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<OfferPaymentBundle> {
    Ok(OfferPaymentBundle {
        requested_payments: build_requested_payments(ctx, terms, receive_puzzle_hash, offer_nonce)?,
        requested_asset_info: AssetInfo::new(),
    })
}

/// Request-side offer assertions plus optional absolute expiry.
///
/// # Errors
///
/// Returns an error if payment assertions cannot be built in the spend context.
pub(crate) fn build_offer_request_conditions(
    ctx: &mut SpendContext,
    payments: &OfferPaymentBundle,
    expires_at: Option<u64>,
) -> SignerResult<Conditions> {
    let assertions = payments
        .requested_payments
        .assertions(ctx, &payments.requested_asset_info)
        .map_err(SignerError::from)?;
    let mut conditions = Conditions::new();
    for assertion in assertions {
        conditions = conditions.with(assertion);
    }
    Ok(apply_offer_expiry(conditions, expires_at))
}

pub(crate) fn apply_offer_expiry(conditions: Conditions, expires_at: Option<u64>) -> Conditions {
    if let Some(seconds) = expires_at {
        conditions.with(AssertBeforeSecondsAbsolute::new(seconds))
    } else {
        conditions
    }
}

pub(crate) fn build_requested_payments(
    ctx: &mut SpendContext,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<RequestedPayments> {
    let mut requested_payments = RequestedPayments::new();
    if is_xch_like_asset(&terms.request_asset_id) {
        requested_payments.xch.push(NotarizedPayment::new(
            offer_nonce,
            vec![Payment::new(
                receive_puzzle_hash,
                terms.request_amount,
                Memos::None,
            )],
        ));
        return Ok(requested_payments);
    }

    let request_asset_id = hex_to_bytes32(&terms.request_asset_id)?;
    let memos = ctx.hint(receive_puzzle_hash).map_err(SignerError::from)?;
    requested_payments.cats.insert(
        request_asset_id,
        vec![NotarizedPayment::new(
            offer_nonce,
            vec![Payment::new(
                receive_puzzle_hash,
                terms.request_amount,
                memos,
            )],
        )],
    );
    Ok(requested_payments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::types::CreateOfferRequest;

    #[test]
    fn recognizes_xch_like_assets() {
        assert!(is_xch_like_asset("xch"));
        assert!(is_xch_like_asset("TXCH"));
        assert!(!is_xch_like_asset("aa".repeat(32).as_str()));
    }

    #[test]
    fn direct_input_requires_split_flag_when_change_without_presplit() {
        assert!(matches!(
            direct_plan_kind_for_amounts(5000, 1000, 1),
            Ok(DirectPlanKind::RequiresSplitFlag)
        ));
        assert!(matches!(
            direct_plan_kind_for_amounts(1000, 1000, 1),
            Ok(DirectPlanKind::Direct)
        ));
        assert!(matches!(
            direct_plan_kind_for_amounts(1000, 1000, 2),
            Err(SignerError::DirectOfferRequiresSingleInputCoin)
        ));
    }

    #[test]
    fn existing_presplit_plan_requires_source_coin_ids_for_nonce() {
        let err = offer_nonce_for_existing_presplit(&[]).unwrap_err();
        assert!(matches!(
            err,
            SignerError::PresplitOfferRequiresSourceCoinIds
        ));
    }

    #[test]
    fn existing_presplit_input_requires_single_presplit_coin() {
        let request = CreateOfferRequest {
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w"
                .to_string(),
            offer_asset_id: hex::encode(Bytes32::new([0x01; 32])),
            offer_amount: 1000,
            request_asset_id: "xch".to_string(),
            request_amount: 1,
            offer_coin_ids: vec![Bytes32::new([0x11; 32])],
            presplit_coin_ids: vec![Bytes32::new([1; 32]), Bytes32::new([2; 32])],
            split_input_coins: false,
            broadcast_split: false,
            expires_at: None,
        };
        let err = OfferInput::try_from(request).unwrap_err();
        assert!(matches!(err, SignerError::PresplitOfferRequiresSingleCoin));
    }

    enum DirectPlanKind {
        Direct,
        RequiresSplitFlag,
    }

    fn direct_plan_kind_for_amounts(
        offered_total: u64,
        offer_amount: u64,
        input_count: usize,
    ) -> Result<DirectPlanKind, SignerError> {
        if offered_total <= offer_amount {
            if input_count != 1 {
                return Err(SignerError::DirectOfferRequiresSingleInputCoin);
            }
            Ok(DirectPlanKind::Direct)
        } else {
            Ok(DirectPlanKind::RequiresSplitFlag)
        }
    }
}

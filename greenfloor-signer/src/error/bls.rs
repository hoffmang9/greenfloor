use super::SignerError;

/// BLS Python FFI operation kind for stable reason strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlsOp {
    MixedSplit,
    Offer,
    XchCoinOp,
    Broadcast,
}

/// Stable snake_case reasons consumed by Python ``bls_signing``.
pub fn bls_reason(err: SignerError, op: BlsOp) -> String {
    match (err, op) {
        (SignerError::MissingOutputAmounts, BlsOp::MixedSplit) => "missing_output_amounts".into(),
        (SignerError::InvalidOutputAmount, BlsOp::MixedSplit) => "invalid_output_amount".into(),
        (SignerError::InvalidOutputAmount, BlsOp::Offer) => "invalid_offer_or_request_amount".into(),
        (SignerError::CatOutputBelowMinimum, BlsOp::MixedSplit) => {
            "cat_output_below_minimum_mojos".into()
        }
        (SignerError::CatChangeBelowMinimum, BlsOp::MixedSplit) => {
            "cat_change_below_minimum_mojos".into()
        }
        (SignerError::NoUnspentXchCoins, BlsOp::MixedSplit) => {
            "no_unspent_xch_coins_for_mixed_split".into()
        }
        (SignerError::NoUnspentXchCoins, BlsOp::XchCoinOp) => "no_unspent_xch_coins".into(),
        (SignerError::XchCoinSelectionFailed, BlsOp::MixedSplit) => {
            "coin_selection_failed_for_mixed_split".into()
        }
        (SignerError::XchCoinSelectionFailed, BlsOp::XchCoinOp) => "coin_selection_failed".into(),
        (SignerError::InsufficientCatCoins, BlsOp::MixedSplit) => {
            "insufficient_cat_coins_for_mixed_split".into()
        }
        (SignerError::InsufficientXchFeeBalanceForMixedSplit, BlsOp::MixedSplit) => {
            "insufficient_xch_fee_balance_for_mixed_split".into()
        }
        (SignerError::NoUnspentOfferXchCoins, BlsOp::Offer) => "no_unspent_offer_xch_coins".into(),
        (SignerError::InsufficientOfferXchCoins, BlsOp::Offer) => {
            "offer_coin_selection_failed".into()
        }
        (SignerError::NoUnspentOfferCatCoins, BlsOp::Offer) => "no_unspent_offer_cat_coins".into(),
        (SignerError::InsufficientOfferCatCoins, BlsOp::Offer) => {
            "insufficient_offer_cat_coins".into()
        }
        (SignerError::UnsupportedOperationType, BlsOp::XchCoinOp) => {
            "unsupported_operation_type".into()
        }
        (SignerError::InvalidPlanValues, BlsOp::XchCoinOp) => "invalid_plan_values".into(),
        (SignerError::InsufficientSelectedCoinTotal, BlsOp::XchCoinOp) => {
            "insufficient_selected_coin_total".into()
        }
        (SignerError::MissingSigningKeyForSelectedCoins, _) => {
            "derivation_scan_failed_for_selected_coin".into()
        }
        (SignerError::UnsupportedNetworkForSigning, _) => "unsupported_network_for_signing".into(),
        (SignerError::Coinset(message), BlsOp::Broadcast) => format!("push_tx_error:{message}"),
        (SignerError::Coinset(message), BlsOp::MixedSplit) => format!("push_tx_error:{message}"),
        (SignerError::Driver(message), BlsOp::MixedSplit) => {
            format!("build_mixed_split_spend_bundle_error:{message}")
        }
        (SignerError::Driver(message), BlsOp::Offer) => {
            format!("build_offer_spend_bundle_error:{message}")
        }
        (SignerError::Driver(message), BlsOp::XchCoinOp) => {
            format!("build_spend_bundle_error:{message}")
        }
        (SignerError::Coinset(message), BlsOp::Offer) => {
            format!("build_offer_spend_bundle_error:{message}")
        }
        (SignerError::Coinset(message), BlsOp::XchCoinOp) => {
            format!("build_spend_bundle_error:{message}")
        }
        (other, BlsOp::MixedSplit) => format!("build_mixed_split_spend_bundle_error:{other}"),
        (other, BlsOp::Offer) => format!("build_offer_spend_bundle_error:{other}"),
        (other, BlsOp::XchCoinOp) => format!("build_spend_bundle_error:{other}"),
        (other, BlsOp::Broadcast) => format!("push_tx_error:{other}"),
    }
}

pub fn mixed_split_reason(err: SignerError) -> String {
    bls_reason(err, BlsOp::MixedSplit)
}

pub fn offer_reason(err: SignerError) -> String {
    bls_reason(err, BlsOp::Offer)
}

pub fn xch_coin_op_reason(err: SignerError) -> String {
    bls_reason(err, BlsOp::XchCoinOp)
}

pub fn broadcast_reason(err: SignerError) -> String {
    bls_reason(err, BlsOp::Broadcast)
}

#[cfg(test)]
mod tests {
    use super::{
        bls_reason, broadcast_reason, mixed_split_reason, offer_reason, xch_coin_op_reason, BlsOp,
        SignerError,
    };

    #[test]
    fn xch_coin_op_reason_maps_stable_python_codes() {
        assert_eq!(
            xch_coin_op_reason(SignerError::NoUnspentXchCoins),
            "no_unspent_xch_coins"
        );
        assert_eq!(
            xch_coin_op_reason(SignerError::InsufficientSelectedCoinTotal),
            "insufficient_selected_coin_total"
        );
    }

    #[test]
    fn offer_reason_maps_stable_python_codes() {
        assert_eq!(
            offer_reason(SignerError::NoUnspentOfferCatCoins),
            "no_unspent_offer_cat_coins"
        );
        assert_eq!(
            offer_reason(SignerError::InvalidOutputAmount),
            "invalid_offer_or_request_amount"
        );
    }

    #[test]
    fn mixed_split_reason_maps_stable_python_codes() {
        assert_eq!(
            mixed_split_reason(SignerError::MissingOutputAmounts),
            "missing_output_amounts"
        );
        assert_eq!(
            mixed_split_reason(SignerError::NoUnspentXchCoins),
            "no_unspent_xch_coins_for_mixed_split"
        );
        assert_eq!(
            mixed_split_reason(SignerError::CatOutputBelowMinimum),
            "cat_output_below_minimum_mojos"
        );
        assert_eq!(
            mixed_split_reason(SignerError::XchCoinSelectionFailed),
            "coin_selection_failed_for_mixed_split"
        );
    }

    #[test]
    fn broadcast_reason_uses_push_tx_prefix() {
        assert_eq!(
            bls_reason(SignerError::Coinset("down".into()), BlsOp::Broadcast),
            "push_tx_error:down"
        );
        assert_eq!(
            broadcast_reason(SignerError::Coinset("down".into())),
            "push_tx_error:down"
        );
    }
}

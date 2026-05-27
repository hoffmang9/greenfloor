use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignerError {
    #[error("cloud wallet private key PEM must live under a .greenfloor directory")]
    PemPathNotUnderDotGreenfloor,

    #[error("cloud wallet private key PEM not found: {0}")]
    PemPathNotFound(String),

    #[error("cloud wallet graphql error: {0}")]
    Graphql(String),

    #[error("vault custody snapshot unavailable")]
    VaultSnapshotUnavailable,

    #[error("vault launcher id missing or invalid")]
    VaultLauncherIdInvalid,

    #[error("vault threshold or timelock invalid")]
    VaultThresholdOrTimelockInvalid,

    #[error("unsupported vault signer cardinality")]
    UnsupportedVaultSignerCardinality,

    #[error("unsupported vault threshold")]
    UnsupportedVaultThreshold,

    #[error("invalid vault recovery timelock")]
    InvalidVaultRecoveryTimelock,

    #[error("unsupported vault curve: {0}")]
    UnsupportedVaultCurve(String),

    #[error("kms public key mismatch: kms={kms} custody={custody}")]
    KmsPublicKeyMismatch { kms: String, custody: String },

    #[error("vault single secp256r1 custody key required, found {0}")]
    VaultSecp256r1KeyCount(usize),

    #[error("missing config field: {0}")]
    MissingConfigField(&'static str),

    #[error("kms error: {0}")]
    Kms(String),

    #[error("coinset error: {0}")]
    Coinset(String),

    #[error("driver error: {0}")]
    Driver(String),

    #[error("no unspent cat coins")]
    NoUnspentCatCoins,

    #[error("insufficient cat coins")]
    InsufficientCatCoins,

    #[error("derivation scan failed for selected coin")]
    MissingSigningKeyForSelectedCoins,

    #[error("no unspent xch coins")]
    NoUnspentXchCoins,

    #[error("insufficient offered total for mixed split")]
    InsufficientOfferedTotalForMixedSplit,

    #[error("insufficient xch fee balance for mixed split")]
    InsufficientXchFeeBalanceForMixedSplit,

    #[error("no unspent offer xch coins")]
    NoUnspentOfferXchCoins,

    #[error("insufficient offer xch coins")]
    InsufficientOfferXchCoins,

    #[error("no unspent offer cat coins")]
    NoUnspentOfferCatCoins,

    #[error("insufficient offer cat coins")]
    InsufficientOfferCatCoins,

    #[error("insufficient offer coin total")]
    InsufficientOfferCoinTotal,

    #[error("unsupported operation type")]
    UnsupportedOperationType,

    #[error("invalid plan values")]
    InvalidPlanValues,

    #[error("insufficient selected coin total")]
    InsufficientSelectedCoinTotal,

    #[error("xch coin selection failed")]
    XchCoinSelectionFailed,

    #[error("unsupported network for signing")]
    UnsupportedNetworkForSigning,

    #[error("cat output below minimum mojos")]
    CatOutputBelowMinimum,

    #[error("cat change below minimum mojos")]
    CatChangeBelowMinimum,

    #[error("vault receive message mode 23 not found")]
    VaultReceiveMessageNotFound,

    #[error("vault singleton coin not found")]
    VaultSingletonNotFound,

    #[error("mixed split vault with fee not supported")]
    MixedSplitVaultWithFeeNotSupported,

    #[error("invalid output amount")]
    InvalidOutputAmount,

    #[error("missing receive address")]
    MissingReceiveAddress,

    #[error("missing asset id")]
    MissingAssetId,

    #[error("missing output amounts")]
    MissingOutputAmounts,

    #[error("presplit requires a single source cat coin")]
    PresplitRequiresSingleSourceCat,

    #[error("offer input exceeds offer amount; enable split-input-coins or specify exact coin")]
    OfferInputRequiresPresplit,

    #[error("presplit coin not found on chain")]
    PresplitCoinNotFound,

    #[error("timeout waiting for presplit coin confirmation")]
    PresplitCoinConfirmationTimeout,

    #[error("presplit offer step requires --offer-coin-ids of original source coins")]
    PresplitOfferRequiresSourceCoinIds,

    #[error("presplit coin amount {coin} does not match offer amount {offer}")]
    PresplitCoinAmountMismatch { coin: u64, offer: u64 },

    #[error("presplit coin asset id does not match offer asset id")]
    PresplitCoinAssetMismatch,

    #[error("presplit offer path supports exactly one presplit coin")]
    PresplitOfferRequiresSingleCoin,

    #[error("presplit coin p2 puzzle hash does not match offer binding")]
    PresplitCoinPuzzleHashMismatch,

    #[error("{0}")]
    Other(String),
}

pub type SignerResult<T> = Result<T, SignerError>;

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
        (SignerError::InsufficientOfferedTotalForMixedSplit, BlsOp::MixedSplit) => {
            "insufficient_offered_total_for_mixed_split".into()
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
        (SignerError::InsufficientOfferCoinTotal, BlsOp::Offer) => {
            "insufficient_offer_coin_total".into()
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
        (SignerError::Coinset(message), _) if op == BlsOp::MixedSplit => {
            format!("push_tx_error:{message}")
        }
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

pub fn driver_error(err: chia_sdk_driver::DriverError) -> SignerError {
    SignerError::Driver(err.to_string())
}

impl From<chia_sdk_driver::DriverError> for SignerError {
    fn from(err: chia_sdk_driver::DriverError) -> Self {
        driver_error(err)
    }
}

impl From<reqwest::Error> for SignerError {
    fn from(err: reqwest::Error) -> Self {
        SignerError::Coinset(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{bls_reason, SignerError, BlsOp};

    #[test]
    fn signer_error_display_messages_are_stable() {
        let cases: Vec<(SignerError, &str)> = vec![
            (
                SignerError::VaultLauncherIdInvalid,
                "vault launcher id missing or invalid",
            ),
            (
                SignerError::InsufficientCatCoins,
                "insufficient cat coins",
            ),
            (
                SignerError::OfferInputRequiresPresplit,
                "offer input exceeds offer amount; enable split-input-coins or specify exact coin",
            ),
            (
                SignerError::PresplitCoinConfirmationTimeout,
                "timeout waiting for presplit coin confirmation",
            ),
            (
                SignerError::KmsPublicKeyMismatch {
                    kms: "aa".to_string(),
                    custody: "bb".to_string(),
                },
                "kms public key mismatch: kms=aa custody=bb",
            ),
            (
                SignerError::MissingConfigField("signer"),
                "missing config field: signer",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn xch_coin_op_reason_maps_stable_python_codes() {
        assert_eq!(
            super::xch_coin_op_reason(SignerError::NoUnspentXchCoins),
            "no_unspent_xch_coins"
        );
        assert_eq!(
            super::xch_coin_op_reason(SignerError::InsufficientSelectedCoinTotal),
            "insufficient_selected_coin_total"
        );
    }

    #[test]
    fn offer_reason_maps_stable_python_codes() {
        assert_eq!(
            super::offer_reason(SignerError::NoUnspentOfferCatCoins),
            "no_unspent_offer_cat_coins"
        );
        assert_eq!(
            super::offer_reason(SignerError::InvalidOutputAmount),
            "invalid_offer_or_request_amount"
        );
    }

    #[test]
    fn mixed_split_reason_maps_stable_python_codes() {
        assert_eq!(
            super::mixed_split_reason(SignerError::MissingOutputAmounts),
            "missing_output_amounts"
        );
        assert_eq!(
            super::mixed_split_reason(SignerError::NoUnspentXchCoins),
            "no_unspent_xch_coins_for_mixed_split"
        );
        assert_eq!(
            super::mixed_split_reason(SignerError::CatOutputBelowMinimum),
            "cat_output_below_minimum_mojos"
        );
        assert_eq!(
            super::mixed_split_reason(SignerError::XchCoinSelectionFailed),
            "coin_selection_failed_for_mixed_split"
        );
    }

    #[test]
    fn broadcast_reason_uses_push_tx_prefix() {
        assert_eq!(
            bls_reason(SignerError::Coinset("down".into()), BlsOp::Broadcast),
            "push_tx_error:down"
        );
    }
}

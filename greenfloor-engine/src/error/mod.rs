use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignerError {
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

    #[error("combine input verify timeout")]
    CombineInputVerifyTimeout,

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

    #[error("offer_missing_expiration")]
    OfferMissingExpiration,

    #[error("offer_duplicate_spent_coin_ids")]
    OfferDuplicateSpentCoinIds,

    #[error("invalid_size_base_units")]
    InvalidSizeBaseUnits,

    #[error("request_amount must be positive")]
    InvalidOfferRequestAmount,

    #[error("invalid ladder math")]
    InvalidLadderMath,

    #[error("invalid_offer_amount")]
    InvalidOfferAmount,

    #[error("signer_asset_resolution_failed:resolved_assets_collide_for_non_xch_pair")]
    ResolvedAssetsCollideForNonXchPair,

    #[error("reservation contention: {0}")]
    ReservationContention(String),

    #[error("managed upstream transient: {0}")]
    ManagedUpstreamTransient(String),

    #[error("database is locked")]
    DatabaseLocked,

    #[error("offer execution requires signer.kms_key_id and vault.launcher_id in program config")]
    SignerPathNotConfigured,

    #[error("daemon_already_running:{path}{detail}")]
    DaemonAlreadyRunning { path: String, detail: String },

    #[error("{0}")]
    Other(String),
}

impl SignerError {
    pub fn is_parallel_dispatch_transient(&self) -> bool {
        match self {
            Self::ReservationContention(_)
            | Self::ManagedUpstreamTransient(_)
            | Self::DatabaseLocked => true,
            Self::Other(message) => {
                let message = message.as_str();
                if message.contains("database is locked") {
                    return true;
                }
                let class = message.split(':').next().unwrap_or(message).trim();
                crate::cycle::is_parallel_dispatch_transient_error(class, message)
                    || crate::cycle::is_transient_managed_upstream_error_text(message)
            }
            _ => false,
        }
    }
}

pub type SignerResult<T> = Result<T, SignerError>;

pub fn driver_error(err: &chia_sdk_driver::DriverError) -> SignerError {
    SignerError::Driver(err.to_string())
}

impl From<chia_sdk_driver::DriverError> for SignerError {
    fn from(err: chia_sdk_driver::DriverError) -> Self {
        driver_error(&err)
    }
}

impl From<reqwest::Error> for SignerError {
    fn from(err: reqwest::Error) -> Self {
        SignerError::Coinset(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::SignerError;

    #[test]
    fn signer_error_display_messages_are_stable() {
        let cases: Vec<(SignerError, &str)> = vec![
            (
                SignerError::VaultLauncherIdInvalid,
                "vault launcher id missing or invalid",
            ),
            (SignerError::InsufficientCatCoins, "insufficient cat coins"),
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
            (
                SignerError::ResolvedAssetsCollideForNonXchPair,
                "signer_asset_resolution_failed:resolved_assets_collide_for_non_xch_pair",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }
}

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

    #[error("failed to resolve cat lineage for coin {0}")]
    CatLineageResolutionFailed(String),

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

fn is_parallel_dispatch_transient_class(exception_class: &str) -> bool {
    matches!(
        exception_class.trim(),
        "ReservationContentionError" | "ManagedUpstreamTransientError" | "TimeoutError"
    )
}

fn is_transient_managed_upstream_error_text(error_text: &str) -> bool {
    const MARKERS: &[&str] = &[
        "timed out",
        "timeout",
        "temporary unavailable",
        "temporarily unavailable",
        "bad gateway",
        "gateway timeout",
        "service unavailable",
        "connection reset",
        "connection refused",
        "managed_offer_http_error:502",
        "managed_offer_http_error:503",
        "managed_offer_http_error:504",
        "managed_offer_network_error",
        "signer_http_error:502",
        "signer_http_error:503",
        "signer_http_error:504",
    ];
    let normalized = error_text.trim().to_ascii_lowercase();
    MARKERS.iter().any(|marker| normalized.contains(marker))
}

impl SignerError {
    #[must_use]
    pub fn is_parallel_dispatch_transient(&self) -> bool {
        match self {
            Self::ReservationContention(_)
            | Self::ManagedUpstreamTransient(_)
            | Self::DatabaseLocked => true,
            Self::Other(message) => {
                let message = message.as_str();
                message.contains("database is locked")
                    || is_parallel_dispatch_transient_class(
                        message.split(':').next().unwrap_or(message).trim(),
                    )
                    || is_transient_managed_upstream_error_text(message)
            }
            _ => false,
        }
    }
}

pub type SignerResult<T> = Result<T, SignerError>;

#[must_use]
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
                SignerError::CatLineageResolutionFailed("abcd".to_string()),
                "failed to resolve cat lineage for coin abcd",
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
            (
                SignerError::ResolvedAssetsCollideForNonXchPair,
                "signer_asset_resolution_failed:resolved_assets_collide_for_non_xch_pair",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn transient_error_text_detects_timeout_markers() {
        assert!(
            SignerError::Other("managed_offer_network_error: connection reset".to_string())
                .is_parallel_dispatch_transient()
        );
        assert!(!SignerError::Other("invalid offer".to_string()).is_parallel_dispatch_transient());
    }

    #[test]
    fn parallel_dispatch_transient_matches_upstream_and_contention_classes() {
        assert!(SignerError::Other("TimeoutError: timed out".to_string())
            .is_parallel_dispatch_transient());
        assert!(
            SignerError::Other("ManagedUpstreamTransientError: timeout".to_string())
                .is_parallel_dispatch_transient()
        );
        assert!(
            SignerError::Other("ReservationContentionError: busy".to_string())
                .is_parallel_dispatch_transient()
        );
        assert!(
            !SignerError::Other("PermanentOfferBuildFailure: bad puzzle".to_string())
                .is_parallel_dispatch_transient()
        );
    }

    #[test]
    fn parallel_dispatch_transient_rejects_non_transient_variants() {
        assert!(
            !SignerError::Driver("invalid mod hash".to_string()).is_parallel_dispatch_transient()
        );
        assert!(!SignerError::InsufficientCatCoins.is_parallel_dispatch_transient());
    }

    #[test]
    fn driver_error_maps_chia_driver_failures() {
        use super::driver_error;
        use chia_sdk_driver::DriverError;

        let mapped = driver_error(&DriverError::InvalidModHash);
        assert!(matches!(mapped, SignerError::Driver(_)));
        assert!(mapped.to_string().contains("invalid mod hash"));

        let from_impl: SignerError = DriverError::InvalidModHash.into();
        assert_eq!(from_impl.to_string(), mapped.to_string());
    }
}

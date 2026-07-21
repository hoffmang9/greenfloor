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

    #[error("unparseable cat lineage: {0}")]
    UnparseableCatLineage(String),

    #[error("no unspent cat coins")]
    NoUnspentCatCoins,

    #[error("insufficient cat coins")]
    InsufficientCatCoins,

    #[error("preselected cat coins do not match requested coin ids")]
    PreselectedCatCoinIdsMismatch,

    #[error("proven dust coin does not match spend-ready cat")]
    ProvenDustCoinMismatch,

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

    #[error("selected mixed split coins are not spendable")]
    MixedSplitSelectedCoinsNotSpendable,

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

    #[error(
        "direct offer requires exactly one input coin equal to offer amount; combine or enable split-input-coins"
    )]
    DirectOfferRequiresSingleInputCoin,

    #[error("presplit coin not found on chain")]
    PresplitCoinNotFound,

    #[error("timeout waiting for presplit coin confirmation")]
    PresplitCoinConfirmationTimeout,

    #[error("combine input verify timeout")]
    CombineInputVerifyTimeout,

    #[error("bootstrap shape wait timeout")]
    BootstrapShapeWaitTimeout,

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

    #[error("offer_cancel_offer_file_not_found")]
    OfferCancelOfferFileNotFound,

    #[error("offer_cancel_offer_file_missing")]
    OfferCancelOfferFileMissing,

    #[error("offer_cancel_no_spendable_input")]
    OfferCancelNoSpendableInput,

    #[error("offer_cancel_input_not_presplit_maker")]
    OfferCancelInputNotPresplitMaker,

    #[error("offer_cancel_input_not_vault_owned: coin={coin_id} puzzle_hash={puzzle_hash} launcher={launcher_id}")]
    OfferCancelInputNotVaultOwned {
        coin_id: String,
        puzzle_hash: String,
        launcher_id: String,
    },

    #[error("offer_cancel_presplit_binding_parse_failed:{detail}")]
    OfferCancelPresplitBindingParseFailed { detail: String },

    #[error("offer_cancel_input_coin_already_spent")]
    OfferCancelInputCoinAlreadySpent,

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

    #[error("failed to open sqlite db {path}: {open_error}")]
    SqliteOpenFailed { path: String, open_error: String },

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

const MIXED_SPLIT_SELECTED_COINS_NOT_SPENDABLE: &str = "Some selected coins are not spendable";

fn mixed_split_selected_coins_not_spendable_message(message: &str) -> bool {
    message.contains(MIXED_SPLIT_SELECTED_COINS_NOT_SPENDABLE)
}

impl SignerError {
    #[must_use]
    pub fn is_mixed_split_selected_coins_not_spendable(&self) -> bool {
        matches!(self, Self::MixedSplitSelectedCoinsNotSpendable)
    }

    #[must_use]
    pub fn normalize_mixed_split_error(err: Self) -> Self {
        if matches!(err, Self::MixedSplitSelectedCoinsNotSpendable) {
            return err;
        }
        if mixed_split_selected_coins_not_spendable_message(&err.to_string()) {
            Self::MixedSplitSelectedCoinsNotSpendable
        } else {
            err
        }
    }

    #[must_use]
    pub fn is_sqlite_fatal(&self) -> bool {
        matches!(self, Self::SqliteOpenFailed { .. })
    }

    #[must_use]
    pub fn is_parallel_dispatch_transient(&self) -> bool {
        if self.is_sqlite_fatal() {
            return false;
        }
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
    fn sqlite_fatal_errors_are_not_parallel_dispatch_transient() {
        assert!(SignerError::SqliteOpenFailed {
            path: "/tmp/greenfloor.sqlite".to_string(),
            open_error: "unable to open database file".to_string(),
        }
        .is_sqlite_fatal());
        assert!(!SignerError::Other("database is locked".to_string()).is_sqlite_fatal());
        assert!(!SignerError::SqliteOpenFailed {
            path: "/tmp/x".to_string(),
            open_error: "permission denied".to_string(),
        }
        .is_parallel_dispatch_transient());
        assert!(SignerError::DatabaseLocked.is_parallel_dispatch_transient());
    }

    #[test]
    fn parallel_dispatch_transient_rejects_non_transient_variants() {
        assert!(
            !SignerError::Driver("invalid mod hash".to_string()).is_parallel_dispatch_transient()
        );
        assert!(!SignerError::InsufficientCatCoins.is_parallel_dispatch_transient());
    }

    #[test]
    fn mixed_split_selected_coins_not_spendable_is_classified() {
        assert!(SignerError::MixedSplitSelectedCoinsNotSpendable
            .is_mixed_split_selected_coins_not_spendable());
        assert!(
            !SignerError::Other("upstream: Some selected coins are not spendable".to_string())
                .is_mixed_split_selected_coins_not_spendable()
        );
        assert!(matches!(
            SignerError::normalize_mixed_split_error(SignerError::Other(
                "Some selected coins are not spendable".to_string()
            )),
            SignerError::MixedSplitSelectedCoinsNotSpendable
        ));
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

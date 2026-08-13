use thiserror::Error;

/// Vault custody, KMS, mixed-split, and vault `CAT` create failures.
#[derive(Debug, Error)]
pub enum VaultError {
    #[error("vault custody snapshot unavailable")]
    SnapshotUnavailable,

    #[error("vault launcher id missing or invalid")]
    LauncherIdInvalid,

    #[error("vault threshold or timelock invalid")]
    ThresholdOrTimelockInvalid,

    #[error("unsupported vault signer cardinality")]
    UnsupportedSignerCardinality,

    #[error("unsupported vault threshold")]
    UnsupportedThreshold,

    #[error("invalid vault recovery timelock")]
    InvalidRecoveryTimelock,

    #[error("unsupported vault curve: {0}")]
    UnsupportedCurve(String),

    #[error("kms public key mismatch: kms={kms} custody={custody}")]
    KmsPublicKeyMismatch { kms: String, custody: String },

    #[error("vault single secp256r1 custody key required, found {0}")]
    Secp256r1KeyCount(usize),

    #[error("kms error: {0}")]
    Kms(String),

    #[error(
        "vault cat create destination is the receive CAT outer puzzle hash (would double-wrap)"
    )]
    CatCreateDestinationIsOuterLayer,

    #[error("vault cat create destination is not the vault receive p2 puzzle hash")]
    CatCreateDestinationNotReceiveP2,

    #[error("vault receive message mode 23 not found")]
    ReceiveMessageNotFound,

    #[error("vault singleton coin not found")]
    SingletonNotFound,

    #[error("mixed split vault with fee not supported")]
    MixedSplitWithFeeNotSupported,

    #[error("selected mixed split coins are not spendable")]
    MixedSplitSelectedCoinsNotSpendable,

    #[error("missing receive address")]
    MissingReceiveAddress,

    #[error("missing asset id")]
    MissingAssetId,

    #[error("missing output amounts")]
    MissingOutputAmounts,

    #[error("invalid output amount")]
    InvalidOutputAmount,

    #[error("unsupported network for signing")]
    UnsupportedNetworkForSigning,
}

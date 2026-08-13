use thiserror::Error;

/// Coin selection, lineage, combine/split plan, and `CAT` dust failures.
#[derive(Debug, Error)]
pub enum CoinOpsError {
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

    #[error("cat output below minimum mojos")]
    CatOutputBelowMinimum,

    #[error("cat change below minimum mojos")]
    CatChangeBelowMinimum,

    #[error("combine input verify timeout")]
    CombineInputVerifyTimeout,

    #[error("bootstrap shape wait timeout")]
    BootstrapShapeWaitTimeout,

    #[error("invalid ladder math")]
    InvalidLadderMath,
}

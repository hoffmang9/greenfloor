use thiserror::Error;

/// Offer construction, presplit, cancel, and asset-resolution failures.
#[derive(Debug, Error)]
pub enum OfferError {
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

    #[error("invalid_offer_amount")]
    InvalidOfferAmount,

    #[error("signer_asset_resolution_failed:resolved_assets_collide_for_non_xch_pair")]
    ResolvedAssetsCollideForNonXchPair,
}

//! Canonical reconcile audit metadata strings for operator logs and batch output.

pub(crate) const SIGNAL_SOURCE_NONE: &str = "none";
pub(crate) const SIGNAL_SOURCE_COINSET_WEBHOOK: &str = "coinset_webhook";
pub(crate) const SIGNAL_SOURCE_COINSET_MEMPOOL: &str = "coinset_mempool";
pub(crate) const SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK: &str = "dexie_status_fallback";
pub(crate) const SIGNAL_SOURCE_DEXIE_GET_OFFER_404: &str = "dexie_get_offer_404";

pub(crate) const REASON_OK: &str = "ok";
pub(crate) const REASON_CANCEL_SUBMIT_STALE_DEXIE_OPEN: &str =
    "cancel_submitted_stale_dexie_still_open";
pub(crate) const REASON_MISSING_STATUS: &str = "missing_status";
pub(crate) const REASON_COINSET_UNAVAILABLE: &str = "coinset_signal_unavailable_for_offer";
pub(crate) const REASON_COINSET_CONFIRMED: &str = "coinset_tx_block_webhook_confirmed";
pub(crate) const REASON_COINSET_MEMPOOL: &str = "coinset_mempool_observed";
pub(crate) const REASON_DEXIE_OFFER_NOT_FOUND: &str = "dexie_offer_not_found";
pub(crate) const REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL: &str =
    "dexie_offer_not_found_preserved_terminal";

pub(crate) const TAKER_NONE: &str = "none";
pub(crate) const TAKER_COINSET_TX_BLOCK_WEBHOOK: &str = "coinset_tx_block_webhook";
pub(crate) const TAKER_DIAGNOSTIC_COINSET_CONFIRMED: &str = "coinset_tx_block_confirmed";
pub(crate) const TAKER_DIAGNOSTIC_COINSET_MEMPOOL: &str = "coinset_mempool_observed";
pub(crate) const TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK: &str = "dexie_status_pattern_fallback";

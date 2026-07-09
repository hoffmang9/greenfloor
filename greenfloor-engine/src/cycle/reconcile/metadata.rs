//! Canonical reconcile audit metadata strings for operator logs and batch output.

pub(crate) const SIGNAL_SOURCE_NONE: &str = "none";
pub(crate) const SIGNAL_SOURCE_COINSET_WEBSOCKET: &str = "coinset_websocket";
pub(crate) const SIGNAL_SOURCE_COINSET_MEMPOOL: &str = "coinset_mempool";
pub(crate) const SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK: &str = "dexie_status_fallback";
pub(crate) const SIGNAL_SOURCE_DEXIE_GET_OFFER_404: &str = "dexie_get_offer_404";
pub(crate) const SIGNAL_SOURCE_CANCEL_TX_CHAIN: &str = "cancel_tx_chain";

pub(crate) const REASON_OK: &str = "ok";
pub(crate) const REASON_POTENTIAL_TAKE_SEEN: &str = "potential_take_seen";
pub(crate) const REASON_TAKE_CONFIRMED_ON_TX_BLOCK: &str = "take_confirmed_on_tx_block";
pub(crate) const REASON_CANCEL_SUBMIT_STALE_ORPHAN: &str = "cancel_submitted_stale_orphan";
pub(crate) const REASON_CANCEL_SUBMIT_CONTEXT_MISSING: &str = "cancel_submitted_context_missing";
pub(crate) const REASON_CANCEL_TX_CHAIN_CONFIRMED: &str = "cancel_tx_chain_confirmed";
pub(crate) const REASON_CANCEL_SUBMIT_WATCH_HIT_IGNORED: &str =
    "cancel_submitted_watch_hit_ignored";
pub(crate) const REASON_CANCEL_SUBMIT_CANCEL_TX_MEMPOOL_IGNORED: &str =
    "cancel_submitted_cancel_tx_mempool_ignored";
pub(crate) const REASON_MISSING_STATUS: &str = "missing_status";
pub(crate) const REASON_COINSET_UNAVAILABLE: &str = "coinset_signal_unavailable_for_offer";
pub(crate) const REASON_COINSET_CONFIRMED: &str = "coinset_tx_block_websocket_confirmed";
pub(crate) const REASON_COINSET_MEMPOOL: &str = "coinset_mempool_observed";
pub(crate) const REASON_DEXIE_OFFER_NOT_FOUND: &str = "dexie_offer_not_found";
pub(crate) const REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL: &str =
    "dexie_offer_not_found_preserved_terminal";

pub(crate) const TAKER_NONE: &str = "none";
pub(crate) const TAKER_COINSET_TX_BLOCK_WEBSOCKET: &str = "coinset_tx_block_websocket";
pub(crate) const TAKER_DIAGNOSTIC_COINSET_CONFIRMED: &str = "coinset_tx_block_confirmed";
pub(crate) const TAKER_DIAGNOSTIC_COINSET_MEMPOOL: &str = "coinset_mempool_observed";
pub(crate) const TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK: &str = "dexie_status_pattern_fallback";
pub(crate) const TAKER_DIAGNOSTIC_CANCEL_TX_CHAIN_CONFIRMED: &str = "cancel_tx_chain_confirmed";

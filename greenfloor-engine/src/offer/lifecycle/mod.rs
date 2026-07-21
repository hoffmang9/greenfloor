//! Offer lifecycle operations shared by manager CLI and daemon cycle.

mod cancel;
mod cancel_cli;
mod cancel_context;
mod cancel_eligibility;
mod persist;
mod reconcile_watched_offers;
mod signal_apply;
mod status_cli;
mod transition;
mod ws_apply;

pub use cancel::{
    cancel_offers_on_chain, cancel_targets_need_dexie_fallback, CancelOfferOnChainParams,
    CancelOfferOutcome, CancelOfferTarget,
};
pub use cancel_cli::{
    offers_cancel_cli, OffersCancelCliItem, OffersCancelCliRequest, OffersCancelCliResult,
};
pub use cancel_context::{cancel_submitted_context_for_offer, CANCEL_SUBMIT_IN_FLIGHT_SKIP_REASON};
pub(crate) use cancel_context::{
    defer_in_flight_cancel_offer_ids, preload_cancel_submitted_contexts,
};
pub use cancel_eligibility::{
    collect_market_cancel_target_offer_ids, filter_cancel_target_offer_ids, row_cancel_eligible,
};
pub use persist::{persist_offer_lifecycle_transition, ReconcilePersistOptions};
pub use reconcile_watched_offers::{
    reconcile_offers_batch, reconcile_offers_cli, ReconcileBatchItem, ReconcileBatchResult,
    ReconcileCliResult,
};
pub use signal_apply::{
    apply_cancel_submitted_rows, apply_signals_to_row, apply_watched_offer_signals,
};
pub use status_cli::{
    offers_status_cli, OfferStatusAuditEvent, OfferStatusRow, OffersStatusCliResult,
};
pub use transition::{
    missing_offer_error_from_payload, resolve_watched_offer_transition_for_venue,
    resolve_watched_offer_transition_from_dexie_fetch, transition_from_dexie_offer_payload,
    transition_from_list_offer_payload, WatchedOfferTransitionEnv,
};
pub use ws_apply::{
    apply_watch_hits_batch, apply_ws_offer_event, promote_cancel_submitted_for_confirmed_txs,
    signals_for_watch_hit, WsOfferApply,
};

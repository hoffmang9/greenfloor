//! Offer lifecycle operations shared by manager CLI and daemon cycle.

mod cancel;
mod cancel_cli;
mod cancel_context;
mod cancel_eligibility;
mod persist;
mod reconcile_watched_offers;
mod status_cli;
mod transition;

pub use cancel::{
    cancel_offer_on_chain, cancel_offers_on_chain, CancelOfferOnChainParams,
    CancelOfferOnChainResult, CancelOfferOutcome, CancelOfferTarget,
};
pub use cancel_cli::{
    offers_cancel_cli, OffersCancelCliItem, OffersCancelCliRequest, OffersCancelCliResult,
};
pub use cancel_context::CANCEL_SUBMIT_IN_FLIGHT_SKIP_REASON;
pub(crate) use cancel_context::{
    defer_in_flight_cancel_offer_ids, preload_cancel_submitted_contexts,
};
pub use cancel_eligibility::{collect_dexie_open_offer_ids, row_cancel_eligible};
pub use persist::{persist_offer_lifecycle_transition, ReconcilePersistOptions};
pub use reconcile_watched_offers::{
    reconcile_offers_batch, reconcile_offers_cli, ReconcileBatchItem, ReconcileBatchResult,
    ReconcileCliResult,
};
pub use status_cli::{
    offers_status_cli, OfferStatusAuditEvent, OfferStatusRow, OffersStatusCliResult,
};
pub use transition::{
    missing_offer_error_from_payload, resolve_watched_offer_transition_for_venue,
    resolve_watched_offer_transition_from_dexie_fetch, transition_from_dexie_offer_payload,
    transition_from_list_offer_payload, WatchedOfferTransitionEnv,
};

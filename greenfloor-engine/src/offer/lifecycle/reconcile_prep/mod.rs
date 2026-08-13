//! Shared reconcile prepare/heal helpers for daemon market cycle and CLI batch paths.
//!
//! Local metadata heal, Dexie watch roles, and metrics-agnostic Dexie fetch live here.
//! Daemon market reconcile wraps watch heal with cycle metrics; CLI batch reconcile uses
//! [`crate::offer::lifecycle::WatchedOfferReconciler::fetch_and_apply`] without heal or
//! cancel-orphan prep.

mod dexie_fetch;
mod watch_plan;

pub use dexie_fetch::{fetch_dexie_offer, fetch_dexie_offer_file_text, DexieOfferFetch};
pub use watch_plan::{
    ensure_watches_from_dexie_payload, fetch_and_ensure_watches, prepare_market_reconcile_local,
};

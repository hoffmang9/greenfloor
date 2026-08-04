//! Shared reconcile prepare/heal helpers for daemon market cycle and CLI batch paths.
//!
//! Local metadata heal, Dexie watch roles, and metrics-agnostic Dexie fetch live here.
//! Daemon market reconcile wraps watch heal with cycle metrics; CLI batch reconcile uses
//! [`fetch_and_apply_watched_offer`] without heal or cancel-orphan prep.

mod dexie_fetch;
mod fetch_apply;
mod watch_plan;

pub use dexie_fetch::{fetch_dexie_offer, DexieOfferFetch};
pub use fetch_apply::fetch_and_apply_watched_offer;
pub use watch_plan::{
    ensure_watches_from_dexie_payload, fetch_and_ensure_watches, prepare_market_reconcile_local,
};

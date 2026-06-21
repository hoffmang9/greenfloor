//! Offer transition policy for daemon cycle signals (taker detection, venue rules).
//!
//! Batch DB reconcile lives in `offer::lifecycle::reconcile_watched_offers`; per-market
//! cycle reconcile lives in `daemon::reconcile_market_cycle`.

mod decision;
mod resolve;
mod state;
mod transition;

#[cfg(test)]
mod tests;

pub use resolve::{
    resolve_missing_watched_offer_transition, resolve_watched_offer_transition_from_signals,
    unchanged_offer_transition, unsupported_venue_offer_transition,
};
pub use transition::CycleOfferTransition;

//! Daemon market-cycle reconcile apply, housed with offer lifecycle policy.
//!
//! Owns Dexie list matching, local watch prepare/heal, augment, and lifecycle
//! transition apply for one market during a daemon cycle. Manager CLI offer-id
//! reconcile remains in [`super::reconcile_watched_offers`]. Daemon still owns
//! schedule, WS transport, and market phase orchestration that invokes this spine.
//!
//! Callers should use [`run_reconcile_market_cycle`] and the result types; prepare /
//! augment / watch helpers stay private to this module.

mod augment;
mod cycle;
mod transition;
mod watch_plan;

#[cfg(test)]
#[path = "cycle_tests.rs"]
mod cycle_tests;

pub use cycle::{run_reconcile_market_cycle, ReconcileMarketCycleResult};
pub use transition::ReconcileMarketCycleMetrics;

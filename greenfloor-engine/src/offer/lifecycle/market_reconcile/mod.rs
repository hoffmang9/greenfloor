//! Market reconcile apply spine shared by daemon cycle and CLI paths.
//!
//! Owns Dexie list matching, local watch prepare/heal, augment, and lifecycle
//! transition apply. Daemon still owns schedule, WS transport, and market phase
//! orchestration that invokes this spine.
//!
//! Callers should use [`run_reconcile_market_cycle`] and the result types; prepare /
//! augment / watch helpers stay crate-private to this module.

mod augment;
mod cycle;
mod transition;
mod watch_plan;

#[cfg(test)]
#[path = "cycle_tests.rs"]
mod cycle_tests;

pub use cycle::{run_reconcile_market_cycle, ReconcileMarketCycleResult};
pub use transition::ReconcileMarketCycleMetrics;

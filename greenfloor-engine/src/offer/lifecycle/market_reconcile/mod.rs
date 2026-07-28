//! Market reconcile apply spine shared by daemon cycle and CLI paths.
//!
//! Owns Dexie list matching, local watch prepare/heal, augment, and lifecycle
//! transition apply. Daemon keeps only schedule + WS transport.

mod augment;
mod cycle;
mod transition;
mod watch_plan;

#[cfg(test)]
#[path = "cycle_tests.rs"]
mod cycle_tests;

pub use augment::augment_dexie_offers_for_watchlist;
pub use cycle::{run_reconcile_market_cycle, ReconcileMarketCycleResult};
pub use transition::ReconcileMarketCycleMetrics;
pub use watch_plan::{
    fetch_and_ensure_watches, prepare_market_reconcile_local, DexieWatchRoles, MarketReconcileLocal,
};

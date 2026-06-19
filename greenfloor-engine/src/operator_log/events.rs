//! Operator trace and audit event name constants.
//!
//! Audit-only paths (`SQLite` `audit_event` only; tracing would duplicate):
//! - Coinset websocket — `coinset_ws_*`
//! - Offer lifecycle reconcile — `offer_lifecycle_transition`
//! - Per-phase audit rows — `strategy_*`, `coin_ops_*`, `inventory_*` (phase boundaries trace `market_phase`)
//! - Cycle preamble — `xch_price_*`, `coinset_mempool_*`
//!
//! Not yet wired:
//! - Low-inventory alerts — `cycle/notifications.rs`
//! - Manager coin-op CLI — `manager_cli/coin_ops/*`

pub const DAEMON_CYCLE_STARTED: &str = "daemon_cycle_started";
pub const DAEMON_CYCLE_COMPLETED: &str = "daemon_cycle_completed";
pub const MARKET_CYCLE_STARTED: &str = "market_cycle_started";
pub const MARKET_CYCLE_COMPLETED: &str = "market_cycle_completed";
pub const MARKET_PHASE: &str = "market_phase";
pub const OFFER_POST_COMPLETED: &str = "offer_post_completed";
pub const OFFER_POST_ITERATION: &str = "offer_post_iteration";
pub const OFFER_POST_FAILURE: &str = "offer_post_failure";
pub const CONFIG_RELOADED: &str = "config_reloaded";
pub const MARKET_VALIDATION_WARNING: &str = "market_validation_warning";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_constants_are_nonempty() {
        assert!(!OFFER_POST_FAILURE.is_empty());
        assert!(!DAEMON_CYCLE_STARTED.is_empty());
    }
}

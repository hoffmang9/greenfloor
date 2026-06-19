//! Operator trace and audit event name constants.
//!
//! Audit-only paths (`SQLite` `audit_event` only; tracing would duplicate):
//! - Coinset websocket — `coinset_ws_*` (live loop / handler)
//! - Offer lifecycle reconcile — `offer_lifecycle_transition`
//! - Per-phase audit rows — `coin_ops_*`, `inventory_*` (phase boundaries trace `market_phase`)
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

pub const XCH_PRICE_SNAPSHOT: &str = "xch_price_snapshot";
pub const XCH_PRICE_ERROR: &str = "xch_price_error";
pub const COINSET_WS_ONCE_ERROR: &str = "coinset_ws_once_error";
pub const COINSET_MEMPOOL_ERROR: &str = "coinset_mempool_error";
pub const COINSET_MEMPOOL_SNAPSHOT: &str = "coinset_mempool_snapshot";
pub const MEMPOOL_OBSERVED: &str = "mempool_observed";

pub const STRATEGY_ACTIONS_PLANNED: &str = "strategy_actions_planned";
pub const STRATEGY_OFFER_EXECUTION: &str = "strategy_offer_execution";
pub const STRATEGY_OFFER_EXECUTION_ERROR: &str = "strategy_offer_execution_error";
pub const STRATEGY_EXEC_SKIPPED_NO_SIGNER: &str = "strategy_exec_skipped_no_signer";
pub const PARALLEL_OFFER_DISPATCH: &str = "parallel_offer_dispatch";
pub const OFFER_PARALLEL_FALLBACK: &str = "offer_parallel_fallback";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_constants_are_nonempty() {
        assert!(!OFFER_POST_FAILURE.is_empty());
        assert!(!DAEMON_CYCLE_STARTED.is_empty());
        assert!(!STRATEGY_ACTIONS_PLANNED.is_empty());
        assert!(!XCH_PRICE_SNAPSHOT.is_empty());
    }
}

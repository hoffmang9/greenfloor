//! Operator trace and audit event name constants.
//!
//! Audit-only paths (`SQLite` `audit_event` only; tracing would duplicate):
//! - Coinset websocket — `coinset_ws_*` (live loop / handler)
//! - Offer lifecycle reconcile — `offer_lifecycle_transition`
//! - Cycle preamble reconcile — `stale_open_offer_requeue_detected`
//!
//! Not yet wired:
//! - Low-inventory alerts — `cycle/notifications.rs`
//! - Manager coin-op CLI — `manager_cli/coin_ops/*`

pub const DAEMON_CYCLE_STARTED: &str = "daemon_cycle_started";
pub const DAEMON_CYCLE_COMPLETED: &str = "daemon_cycle_completed";
pub const DAEMON_CYCLE_SUMMARY: &str = "daemon_cycle_summary";
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

pub const INVENTORY_BUCKET_SCAN: &str = "inventory_bucket_scan";
pub const INVENTORY_BUCKET_SCAN_ERROR: &str = "inventory_bucket_scan_error";

pub const STRATEGY_ACTIONS_PLANNED: &str = "strategy_actions_planned";
pub const STRATEGY_OFFER_EXECUTION: &str = "strategy_offer_execution";
pub const STRATEGY_OFFER_EXECUTION_ERROR: &str = "strategy_offer_execution_error";
pub const STRATEGY_EXEC_SKIPPED_NO_SIGNER: &str = "strategy_exec_skipped_no_signer";
pub const PARALLEL_OFFER_DISPATCH: &str = "parallel_offer_dispatch";
pub const OFFER_PARALLEL_FALLBACK: &str = "offer_parallel_fallback";

pub const OFFER_CANCEL_POLICY: &str = "offer_cancel_policy";

pub const COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT: &str = "coin_ops_skip_sub_minimum_target_amount";
pub const COIN_OPS_INVALID_LADDER_MATH: &str = "coin_ops_invalid_ladder_math";
pub const COIN_OPS_NO_PLANS: &str = "coin_ops_no_plans";
pub const COIN_OPS_PARTIAL_OR_SKIPPED_FEE_BUDGET: &str = "coin_ops_partial_or_skipped_fee_budget";
pub const COIN_OPS_PLAN: &str = "coin_ops_plan";
pub const COIN_OPS_SKIPPED_FEE_BUDGET: &str = "coin_ops_skipped_fee_budget";
pub const COIN_OPS_EXECUTED: &str = "coin_ops_executed";

pub const OFFER_LIFECYCLE_TRANSITION: &str = "offer_lifecycle_transition";
pub const OFFER_RECONCILIATION: &str = "offer_reconciliation";
pub const TAKER_DETECTION: &str = "taker_detection";
pub const DEXIE_OFFERS_ERROR: &str = "dexie_offers_error";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_constants_are_nonempty() {
        assert!(!OFFER_POST_FAILURE.is_empty());
        assert!(!DAEMON_CYCLE_STARTED.is_empty());
        assert!(!STRATEGY_ACTIONS_PLANNED.is_empty());
        assert!(!XCH_PRICE_SNAPSHOT.is_empty());
        assert!(!COIN_OPS_PLAN.is_empty());
        assert!(!INVENTORY_BUCKET_SCAN.is_empty());
    }
}

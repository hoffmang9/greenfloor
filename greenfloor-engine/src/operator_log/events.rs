use tracing::Level;

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

pub const COINSET_WS_RECOVERY_POLL: &str = "coinset_ws_recovery_poll";
pub const COINSET_WS_RECOVERY_POLL_ERROR: &str = "coinset_ws_recovery_poll_error";
pub const COINSET_WS_PAYLOAD_PARSE_ERROR: &str = "coinset_ws_payload_parse_error";
pub const COINSET_WS_PAYLOAD_IGNORED: &str = "coinset_ws_payload_ignored";
pub const COINSET_WS_MEMPOOL_EVENT: &str = "coinset_ws_mempool_event";
pub const COINSET_WS_TX_BLOCK_EVENT: &str = "coinset_ws_tx_block_event";
pub const TX_BLOCK_CONFIRMED: &str = "tx_block_confirmed";
pub const COINSET_WS_COIN_OBSERVED: &str = "coinset_ws_coin_observed";
pub const COIN_WATCH_HIT: &str = "coin_watch_hit";
pub const COINSET_WS_ONCE_STARTED: &str = "coinset_ws_once_started";
pub const COINSET_WS_ONCE_CONNECTED: &str = "coinset_ws_once_connected";
pub const COINSET_WS_ONCE_DISCONNECTED: &str = "coinset_ws_once_disconnected";
pub const COINSET_WS_CONNECTING: &str = "coinset_ws_connecting";
pub const COINSET_WS_CONNECTED: &str = "coinset_ws_connected";
pub const COINSET_WS_DISCONNECTED: &str = "coinset_ws_disconnected";
pub const COIN_WATCHLIST_UPDATED: &str = "coin_watchlist_updated";

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

pub const MARKET_CYCLE_ERROR: &str = "market_cycle_error";

pub const COIN_OP_LEDGER_EXECUTED: &str = "coin_op_executed";
pub const COIN_OP_LEDGER_SKIPPED: &str = "coin_op_skipped";
pub const COIN_OP_LEDGER_PLANNED: &str = "coin_op_planned";

pub const OFFER_LIFECYCLE_TRANSITION: &str = "offer_lifecycle_transition";
pub const OFFER_RECONCILIATION: &str = "offer_reconciliation";
pub const TAKER_DETECTION: &str = "taker_detection";
pub const DEXIE_OFFERS_ERROR: &str = "dexie_offers_error";
pub const DEXIE_WATCHLIST_AUGMENT_ERROR: &str = "dexie_watchlist_augment_error";
pub const STALE_OPEN_OFFER_REQUEUE_DETECTED: &str = "stale_open_offer_requeue_detected";

pub const HOME_BOOTSTRAP: &str = "home_bootstrap";
pub const DOCTOR_PING: &str = "doctor_ping";

#[must_use]
pub fn coin_op_ledger_event(status: &str) -> (&'static str, Level) {
    match status {
        "executed" => (COIN_OP_LEDGER_EXECUTED, Level::INFO),
        "planned" => (COIN_OP_LEDGER_PLANNED, Level::DEBUG),
        _ => (COIN_OP_LEDGER_SKIPPED, Level::DEBUG),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_constants_are_nonempty() {
        assert!(!OFFER_POST_FAILURE.is_empty());
        assert!(!COINSET_WS_MEMPOOL_EVENT.is_empty());
    }

    #[test]
    fn coin_op_ledger_event_maps_known_statuses() {
        assert_eq!(
            coin_op_ledger_event("executed"),
            (COIN_OP_LEDGER_EXECUTED, Level::INFO)
        );
    }
}

//! Structured operator logging: tracing field discipline, redaction, and audit dual-emit.
//!
//! **Tracing** → `{home_dir}/logs/debug.log` via [`trace_event!`].
//! **Audit** → `SQLite` `audit_event` via [`audit_and_trace`].
//!
//! Event name constants live in [`events`].

mod emit;
mod events;
mod redact;

pub use emit::{
    audit_and_trace, audit_config, audit_daemon_cycle, audit_daemon_cycle_only, audit_market_cycle,
    audit_only, trace_audit_outcome, LogContext,
};

#[cfg(test)]
pub use emit::trace_capture::TraceCapture;
pub use events::{
    COINSET_MEMPOOL_ERROR, COINSET_MEMPOOL_SNAPSHOT, COINSET_WS_ONCE_ERROR, COIN_OPS_EXECUTED,
    COIN_OPS_INVALID_LADDER_MATH, COIN_OPS_NO_PLANS, COIN_OPS_PARTIAL_OR_SKIPPED_FEE_BUDGET,
    COIN_OPS_PLAN, COIN_OPS_SKIPPED_FEE_BUDGET, COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT,
    CONFIG_RELOADED, DAEMON_CYCLE_COMPLETED, DAEMON_CYCLE_STARTED, DAEMON_CYCLE_SUMMARY,
    DEXIE_OFFERS_ERROR, DEXIE_WATCHLIST_AUGMENT_ERROR, INVENTORY_BUCKET_SCAN,
    INVENTORY_BUCKET_SCAN_ERROR, MARKET_CYCLE_COMPLETED, MARKET_CYCLE_STARTED, MARKET_PHASE,
    MARKET_VALIDATION_WARNING, MEMPOOL_OBSERVED, OFFER_CANCEL_POLICY, OFFER_LIFECYCLE_TRANSITION,
    OFFER_PARALLEL_FALLBACK, OFFER_POST_COMPLETED, OFFER_POST_FAILURE, OFFER_POST_ITERATION,
    OFFER_RECONCILIATION, PARALLEL_OFFER_DISPATCH, STALE_OPEN_OFFER_REQUEUE_DETECTED,
    STRATEGY_ACTIONS_PLANNED, STRATEGY_EXEC_SKIPPED_NO_SIGNER, STRATEGY_OFFER_EXECUTION,
    STRATEGY_OFFER_EXECUTION_ERROR, TAKER_DETECTION, XCH_PRICE_ERROR, XCH_PRICE_SNAPSHOT,
};
pub use redact::{offer_log_ref, redact_json_for_log, truncate_id};

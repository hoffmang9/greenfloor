//! Structured operator logging: tracing field discipline, redaction, and audit dual-emit.
//!
//! **Tracing** → `{home_dir}/logs/debug.log` via [`trace_event!`].
//! **Audit** → `SQLite` `audit_event` via [`audit_and_trace`].
//!
//! Event name constants live in [`events`].

mod emit;
mod events;
mod redact;

pub use emit::{audit_and_trace, trace_audit_outcome, LogContext};

#[cfg(test)]
pub use emit::trace_capture::TraceCapture;
pub use events::{
    COINSET_MEMPOOL_ERROR, COINSET_MEMPOOL_SNAPSHOT, COINSET_WS_ONCE_ERROR, CONFIG_RELOADED,
    DAEMON_CYCLE_COMPLETED, DAEMON_CYCLE_STARTED, MARKET_CYCLE_COMPLETED, MARKET_CYCLE_STARTED,
    MARKET_PHASE, MARKET_VALIDATION_WARNING, MEMPOOL_OBSERVED, OFFER_PARALLEL_FALLBACK,
    OFFER_POST_COMPLETED, OFFER_POST_FAILURE, OFFER_POST_ITERATION, PARALLEL_OFFER_DISPATCH,
    STRATEGY_ACTIONS_PLANNED, STRATEGY_EXEC_SKIPPED_NO_SIGNER, STRATEGY_OFFER_EXECUTION,
    STRATEGY_OFFER_EXECUTION_ERROR, XCH_PRICE_ERROR, XCH_PRICE_SNAPSHOT,
};
pub use redact::{offer_log_ref, redact_json_for_log, truncate_id};

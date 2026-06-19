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
    CONFIG_RELOADED, DAEMON_CYCLE_COMPLETED, DAEMON_CYCLE_STARTED, MARKET_CYCLE_COMPLETED,
    MARKET_CYCLE_STARTED, MARKET_PHASE, MARKET_VALIDATION_WARNING, OFFER_POST_COMPLETED,
    OFFER_POST_FAILURE, OFFER_POST_ITERATION,
};
pub use redact::{offer_log_ref, redact_json_for_log, truncate_id};

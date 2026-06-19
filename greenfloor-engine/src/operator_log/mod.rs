//! Structured operator logging: tracing field discipline, redaction, and audit dual-emit.
//!
//! **Tracing** → `{home_dir}/logs/debug.log` for tailing live operator behavior.
//! **Audit** → `SQLite` `audit_event` for durable query (`offers-status`, integration tests).
//!
//! See [`inventory`] for the event catalog and documented gaps.

mod emit;
mod inventory;
mod redact;

pub use emit::{audit_and_trace, debug, error, info, warn, LogContext};
pub use inventory::{
    audit_only, not_wired, tracing_inventory, InventoryEntry, CONFIG_RELOADED,
    DAEMON_CYCLE_COMPLETED, DAEMON_CYCLE_STARTED, MARKET_CYCLE_COMPLETED, MARKET_CYCLE_STARTED,
    MARKET_PHASE, MARKET_VALIDATION_WARNING, OFFER_POST_COMPLETED, OFFER_POST_ITERATION,
};
pub use redact::{offer_log_ref, redact_field, redact_json_for_log, truncate_id};

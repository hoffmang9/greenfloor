//! Structured operator logging: tracing field discipline, redaction, and audit dual-emit.
//!
//! **Tracing** → `{home_dir}/logs/debug.log`.
//! **Audit** → `SQLite` `audit_event` via [`audit_and_trace`] and scoped helpers.
//!
//! ## Trace shape tiers (one contract per call site)
//!
//! 1. **Structured boundary** — [`trace_event!`] or [`event_at_level!`]: explicit fields for
//!    cycle/phase boundaries and completion lines. Never dual-emit a second trace for the same
//!    boundary.
//! 2. **Dual-emit mirror** — [`audit_market_cycle`], [`audit_daemon_cycle`], etc.: persist the
//!    full audit row, then mirror to trace via [`trace_audit_mirror`]:
//!    - **Structured mirror** when every top-level payload value is a scalar and the redacted
//!      JSON is ≤512 bytes (`error`, `action_count`, `price_usd`, … as tracing fields).
//!    - **Blob mirror** when the payload contains nested arrays/objects or exceeds the size
//!      limit — one redacted `payload` JSON field ([`trace_audit_outcome`]).
//! 3. **Audit-only** — [`audit_only`], [`audit_daemon_cycle_only`]: `SQLite` row only (high-volume
//!    rows or payloads superseded by a tier-1 boundary trace). Document in [`events`].
//!
//! Event name constants live in [`events`].

mod emit;
mod events;
mod redact;
mod trace_mirror;

pub use emit::{
    audit_and_trace, audit_config, audit_daemon_cycle, audit_daemon_cycle_only, audit_market_cycle,
    audit_only, trace_audit_mirror, trace_audit_outcome, LogContext,
};
pub use trace_mirror::payload_use_blob_mirror;

#[cfg(test)]
pub use emit::trace_capture::TraceCapture;
pub use events::coin_op_ledger_event;
pub use events::{
    COINSET_MEMPOOL_ERROR, COINSET_MEMPOOL_SNAPSHOT, COINSET_WS_ONCE_ERROR, COIN_OPS_EXECUTED,
    COIN_OPS_INVALID_LADDER_MATH, COIN_OPS_NO_PLANS, COIN_OPS_PARTIAL_OR_SKIPPED_FEE_BUDGET,
    COIN_OPS_PLAN, COIN_OPS_SKIPPED_FEE_BUDGET, COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT,
    COIN_OP_LEDGER_EXECUTED, COIN_OP_LEDGER_PLANNED, COIN_OP_LEDGER_SKIPPED, CONFIG_RELOADED,
    DAEMON_CYCLE_COMPLETED, DAEMON_CYCLE_STARTED, DAEMON_CYCLE_SUMMARY, DEXIE_OFFERS_ERROR,
    DEXIE_WATCHLIST_AUGMENT_ERROR, INVENTORY_BUCKET_SCAN, INVENTORY_BUCKET_SCAN_ERROR,
    MARKET_CYCLE_COMPLETED, MARKET_CYCLE_ERROR, MARKET_CYCLE_STARTED, MARKET_PHASE,
    MARKET_VALIDATION_WARNING, MEMPOOL_OBSERVED, OFFER_CANCEL_POLICY, OFFER_LIFECYCLE_TRANSITION,
    OFFER_PARALLEL_FALLBACK, OFFER_POST_COMPLETED, OFFER_POST_FAILURE, OFFER_POST_ITERATION,
    OFFER_RECONCILIATION, PARALLEL_OFFER_DISPATCH, STALE_OPEN_OFFER_REQUEUE_DETECTED,
    STRATEGY_ACTIONS_PLANNED, STRATEGY_EXEC_SKIPPED_NO_SIGNER, STRATEGY_OFFER_EXECUTION,
    STRATEGY_OFFER_EXECUTION_ERROR, TAKER_DETECTION, XCH_PRICE_ERROR, XCH_PRICE_SNAPSHOT,
};
pub use redact::{offer_log_ref, redact_json_for_log, truncate_id};

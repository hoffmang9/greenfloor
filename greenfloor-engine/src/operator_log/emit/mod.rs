//! Audit persistence, trace mirrors, and deferred dual emit.

mod audit;
mod context;
mod deferred;

pub use context::{AuditDurability, LogContext};
pub use deferred::{audit_row_defer_dual, emit_deferred_dual_traces, DeferredDualEmit};

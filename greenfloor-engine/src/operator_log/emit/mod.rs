//! Audit persistence, trace mirrors, and deferred dual emit.

mod audit;
mod context;
mod deferred;

#[cfg(test)]
mod tests;

pub use context::LogContext;
pub(crate) use deferred::{audit_row_defer_dual, emit_deferred_dual_traces, DeferredDualEmit};

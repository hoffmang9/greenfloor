//! Operator logging: audit rows, trace mirrors, redaction.

mod emit;
mod events;
mod macros;
mod redact;

#[cfg(test)]
pub mod test_util;

pub use emit::LogContext;
pub(crate) use emit::{audit_row_defer_dual, emit_deferred_dual_traces, DeferredDualEmit};

pub use events::coin_op_ledger_event;
pub use events::*;
pub use redact::{offer_log_ref, redact_json_for_log, truncate_id};
#[cfg(test)]
pub use test_util::TraceCapture;

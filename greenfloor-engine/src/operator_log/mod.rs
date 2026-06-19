//! Operator logging: audit rows, trace mirrors, redaction.

mod emit;
mod events;
mod redact;

#[cfg(test)]
pub mod test_util;

pub use emit::{
    audit_row, operator_audit, trace_audit_mirror, AuditDurability, EmitMode, LogContext,
};

pub use events::coin_op_ledger_event;
pub use events::*;
pub use redact::{offer_log_ref, redact_json_for_log, truncate_id};
#[cfg(test)]
pub use test_util::TraceCapture;

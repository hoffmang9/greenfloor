use serde_json::Value;
use tracing::Level;

use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::audit::{persist_only, trace_payload_mirror};
use super::context::{AuditDurability, LogContext};

pub struct DeferredDualEmit {
    pub ctx: LogContext,
    pub level: Level,
    pub trace_message: &'static str,
    pub audit_event_type: &'static str,
    pub payload: Value,
    pub market_id: Option<String>,
}

/// Persist an audit row during a transaction and queue its trace mirror for after commit.
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn audit_row_defer_dual(
    deferred: &mut Vec<DeferredDualEmit>,
    store: &SqliteStore,
    entry: DeferredDualEmit,
) -> SignerResult<()> {
    persist_only(
        store,
        entry.audit_event_type,
        &entry.payload,
        entry.market_id.as_deref(),
        AuditDurability::Required,
    )?;
    deferred.push(entry);
    Ok(())
}

pub fn emit_deferred_dual_traces(deferred: &[DeferredDualEmit]) {
    for entry in deferred {
        trace_payload_mirror(
            entry.level,
            entry.ctx,
            entry.audit_event_type,
            &entry.payload,
            entry.market_id.as_deref(),
            entry.trace_message,
        );
    }
}

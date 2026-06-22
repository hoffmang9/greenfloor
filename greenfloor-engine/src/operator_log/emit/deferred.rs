use serde_json::Value;
use tracing::Level;

use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::audit::{persist_only, trace_payload_mirror};
use super::context::{AuditDurability, DualAudit, LogContext};

pub(crate) struct DeferredDualEmit {
    ctx: LogContext,
    level: Level,
    trace_message: &'static str,
    event_type: &'static str,
    payload: Value,
    market_id: Option<String>,
}

impl DeferredDualEmit {
    pub(crate) fn new(
        ctx: LogContext,
        level: Level,
        trace_message: &'static str,
        event_type: &'static str,
        payload: Value,
        market_id: Option<String>,
    ) -> Self {
        Self {
            ctx,
            level,
            trace_message,
            event_type,
            payload,
            market_id,
        }
    }

    fn audit(&self) -> DualAudit<'_> {
        let audit = DualAudit::new(
            self.level,
            self.trace_message,
            self.event_type,
            &self.payload,
        );
        match self.market_id.as_deref() {
            Some(market_id) => audit.with_market_id(market_id),
            None => audit,
        }
    }
}

/// Persist an audit row during a transaction and queue its trace mirror for after commit.
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub(crate) fn audit_row_defer_dual(
    deferred: &mut Vec<DeferredDualEmit>,
    store: &SqliteStore,
    entry: DeferredDualEmit,
) -> SignerResult<()> {
    let audit = entry.audit();
    persist_only(
        store,
        audit.event_type(),
        audit.payload(),
        audit.market_id(),
        AuditDurability::Required,
    )?;
    deferred.push(entry);
    Ok(())
}

pub(crate) fn emit_deferred_dual_traces(deferred: &[DeferredDualEmit]) {
    for entry in deferred {
        trace_payload_mirror(entry.ctx, &entry.audit());
    }
}

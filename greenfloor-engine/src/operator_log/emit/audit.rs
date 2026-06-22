use serde_json::Value;

use crate::error::SignerResult;
use crate::operator_log::redact::redact_json_for_log;
use crate::storage::SqliteStore;

use super::context::{AuditDurability, DualAudit, LogContext};

pub(crate) fn persist_only(
    store: &SqliteStore,
    event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
    durability: AuditDurability,
) -> SignerResult<()> {
    match store.add_audit_event(event_type, payload, market_id) {
        Ok(()) => Ok(()),
        Err(err) if durability == AuditDurability::Required => Err(err),
        Err(err) => {
            tracing::warn!(
                event = event_type,
                error = %err,
                "operator audit persist failed"
            );
            Ok(())
        }
    }
}

pub(crate) fn persist_and_mirror(
    store: &SqliteStore,
    ctx: LogContext,
    audit: &DualAudit<'_>,
) -> SignerResult<()> {
    persist_only(
        store,
        audit.event_type(),
        audit.payload(),
        audit.market_id(),
        AuditDurability::Required,
    )?;
    trace_payload_mirror(ctx, audit);
    Ok(())
}

pub(crate) fn trace_payload_mirror(ctx: LogContext, audit: &DualAudit<'_>) {
    let payload_text = redact_json_for_log(audit.payload()).to_string();
    crate::event_at_level!(
        audit.level(),
        service = ctx.service(),
        event = audit.event_type(),
        phase = ctx.phase(),
        market_id = audit.market_id().unwrap_or(""),
        message = audit.trace_message(),
        payload = %payload_text,
        "operator audit mirrored"
    );
}

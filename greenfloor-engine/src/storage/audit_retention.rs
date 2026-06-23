//! Retention policy and prune orchestration for append-only `audit_event` rows.

use std::sync::LazyLock;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

use crate::cycle::lifecycle::OfferLifecycleState;
use crate::cycle::periodic::{PeriodicGate, PeriodicOutcome};
use crate::cycle::reconcile::{
    REASON_CANCEL_TX_CHAIN_CONFIRMED, REASON_COINSET_CONFIRMED, REASON_COINSET_MEMPOOL, REASON_OK,
    REASON_POTENTIAL_TAKE_SEEN, REASON_TAKE_CONFIRMED_ON_TX_BLOCK, STATE_CANCELLED,
};
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{
    COIN_OPS_EXECUTED, COIN_OP_LEDGER_EXECUTED, OFFER_CANCEL_POLICY, OFFER_LIFECYCLE_TRANSITION,
    TAKER_DETECTION,
};
use crate::storage::SqliteStore;

pub const DEFAULT_AUDIT_RETENTION_DAYS: u64 = 30;
pub const DEFAULT_AUDIT_PRUNE_INTERVAL_SECONDS: u64 = 86_400;
pub const DEFAULT_AUDIT_PRUNE_FAILURE_RETRY_SECONDS: u64 = 3_600;
pub const DEFAULT_AUDIT_PRUNE_BATCH_SIZE: u64 = 10_000;

const PRESERVED_EVENT_TYPES: &[&str] =
    &[TAKER_DETECTION, COIN_OP_LEDGER_EXECUTED, COIN_OPS_EXECUTED];

const PRESERVED_LIFECYCLE_TRANSITIONS: &[(&str, &[&str])] = &[
    (
        OfferLifecycleState::MempoolObserved.as_str(),
        &[REASON_POTENTIAL_TAKE_SEEN, REASON_COINSET_MEMPOOL],
    ),
    (
        OfferLifecycleState::TxBlockConfirmed.as_str(),
        &[REASON_TAKE_CONFIRMED_ON_TX_BLOCK, REASON_COINSET_CONFIRMED],
    ),
    (
        STATE_CANCELLED,
        &[REASON_CANCEL_TX_CHAIN_CONFIRMED, REASON_OK],
    ),
];

static PRESERVE_PREDICATE_SQL: LazyLock<String> = LazyLock::new(build_preserve_predicate_sql);
static AUDIT_PRUNE_GATE: LazyLock<PeriodicGate> = LazyLock::new(PeriodicGate::new);

#[derive(Debug, Clone, Copy)]
pub struct PruneAuditEventsOptions {
    pub dry_run: bool,
    pub vacuum: bool,
    pub batch_size: u64,
}

impl PruneAuditEventsOptions {
    #[must_use]
    pub fn daemon() -> Self {
        Self {
            dry_run: false,
            vacuum: false,
            batch_size: DEFAULT_AUDIT_PRUNE_BATCH_SIZE,
        }
    }

    #[must_use]
    pub fn cli(dry_run: bool, vacuum: bool) -> Self {
        Self {
            dry_run,
            vacuum,
            batch_size: DEFAULT_AUDIT_PRUNE_BATCH_SIZE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneAuditEventsReport {
    pub retention_days: u64,
    pub cutoff: DateTime<Utc>,
    pub dry_run: bool,
    pub deletable_count: u64,
    pub deleted_count: u64,
    pub vacuum_ran: bool,
}

/// Compute the cutoff timestamp for non-financial audit rows.
///
/// # Errors
///
/// Returns an error when `retention_days` cannot be represented as an `i64` day count.
pub fn audit_retention_cutoff(retention_days: u64) -> SignerResult<DateTime<Utc>> {
    let days = i64::try_from(retention_days).map_err(|_| {
        SignerError::Other(format!(
            "storage.audit_retention_days exceeds i64 max: {retention_days}"
        ))
    })?;
    Ok(Utc::now() - Duration::days(days))
}

#[must_use]
pub fn preserve_predicate_sql() -> &'static str {
    &PRESERVE_PREDICATE_SQL
}

#[must_use]
pub fn is_preserved_audit_row(event_type: &str, payload: &Value) -> bool {
    if PRESERVED_EVENT_TYPES.contains(&event_type) {
        return true;
    }
    if event_type == OFFER_CANCEL_POLICY {
        return payload
            .get("executed_count")
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|count| count > 0);
    }
    if event_type != OFFER_LIFECYCLE_TRANSITION {
        return false;
    }
    let Some(new_state) = payload.get("new_state").and_then(Value::as_str) else {
        return false;
    };
    let reason = payload
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    PRESERVED_LIFECYCLE_TRANSITIONS
        .iter()
        .any(|(state, reasons)| *state == new_state && reasons.contains(&reason))
}

#[must_use]
pub fn audit_prune_interval_seconds() -> u64 {
    std::env::var("GREENFLOOR_AUDIT_PRUNE_INTERVAL_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_AUDIT_PRUNE_INTERVAL_SECONDS)
        .max(3_600)
}

#[must_use]
pub fn audit_prune_failure_retry_seconds() -> u64 {
    std::env::var("GREENFLOOR_AUDIT_PRUNE_FAILURE_RETRY_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_AUDIT_PRUNE_FAILURE_RETRY_SECONDS)
        .max(3_600)
}

#[must_use]
pub(crate) fn audit_prune_periodic_outcome(
    result: &SignerResult<PruneAuditEventsReport>,
) -> PeriodicOutcome {
    match result {
        Ok(_) => PeriodicOutcome::Completed,
        Err(_) => PeriodicOutcome::RetryAfter(audit_prune_failure_retry_seconds()),
    }
}

/// Best-effort daily prune hook for the daemon cycle. Failures are logged and ignored.
pub fn maybe_prune_stale_audit_events(store: &SqliteStore, retention_days: u64) {
    let interval_seconds = audit_prune_interval_seconds();
    AUDIT_PRUNE_GATE.run_if_due(interval_seconds, || {
        let options = PruneAuditEventsOptions::daemon();
        let result = store.prune_stale_audit_events(retention_days, options);
        if let Ok(ref report) = result {
            if report.deleted_count > 0 {
                tracing::info!(
                    deleted = report.deleted_count,
                    retention_days = report.retention_days,
                    cutoff = %report.cutoff.to_rfc3339(),
                    interval_seconds,
                    event = "audit_event_pruned",
                    "pruned non-financial audit events"
                );
            }
        } else if let Err(ref err) = result {
            tracing::warn!(
                error = %err,
                retention_days,
                retry_after_seconds = audit_prune_failure_retry_seconds(),
                event = "audit_event_prune_failed",
                "audit retention prune failed; continuing daemon cycle"
            );
        }
        audit_prune_periodic_outcome(&result)
    });
}

fn build_preserve_predicate_sql() -> String {
    let event_types = PRESERVED_EVENT_TYPES
        .iter()
        .map(|event_type| format!("'{event_type}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let lifecycle = PRESERVED_LIFECYCLE_TRANSITIONS
        .iter()
        .map(|(state, reasons)| {
            let reason_list = reasons
                .iter()
                .map(|reason| format!("'{reason}'"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "(json_extract(payload_json, '$.new_state') = '{state}' \
                 AND json_extract(payload_json, '$.reason') IN ({reason_list}))"
            )
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    format!(
        r"
        event_type IN ({event_types})
        OR (
            event_type = '{OFFER_LIFECYCLE_TRANSITION}'
            AND ({lifecycle})
        )
        OR (
            event_type = '{OFFER_CANCEL_POLICY}'
            AND COALESCE(json_extract(payload_json, '$.executed_count'), 0) > 0
        )
        "
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserve_predicate_sql_includes_take_cancel_and_coin_ops() {
        let sql = preserve_predicate_sql();
        assert!(sql.contains(TAKER_DETECTION));
        assert!(sql.contains(COIN_OP_LEDGER_EXECUTED));
        assert!(sql.contains("cancel_tx_chain_confirmed"));
        assert!(sql.contains(OFFER_CANCEL_POLICY));
    }

    #[test]
    fn is_preserved_audit_row_matches_lifecycle_and_policy_cases() {
        assert!(is_preserved_audit_row(
            OFFER_LIFECYCLE_TRANSITION,
            &json!({"new_state":"cancelled","reason":"cancel_tx_chain_confirmed"}),
        ));
        assert!(is_preserved_audit_row(
            OFFER_LIFECYCLE_TRANSITION,
            &json!({"new_state":"cancelled","reason":"ok"}),
        ));
        assert!(is_preserved_audit_row(
            OFFER_LIFECYCLE_TRANSITION,
            &json!({"new_state":"tx_block_confirmed","reason":"take_confirmed_on_tx_block"}),
        ));
        assert!(is_preserved_audit_row(
            OFFER_CANCEL_POLICY,
            &json!({"executed_count": 1}),
        ));
        assert!(!is_preserved_audit_row(
            OFFER_LIFECYCLE_TRANSITION,
            &json!({"new_state":"open","reason":"ok"}),
        ));
        assert!(!is_preserved_audit_row(
            OFFER_CANCEL_POLICY,
            &json!({"executed_count": 0}),
        ));
    }

    #[test]
    fn audit_prune_periodic_outcome_retries_after_failure() {
        let err = Err(SignerError::Other("prune failed".to_string()));
        assert_eq!(
            audit_prune_periodic_outcome(&err),
            PeriodicOutcome::RetryAfter(DEFAULT_AUDIT_PRUNE_FAILURE_RETRY_SECONDS)
        );

        let ok = Ok(PruneAuditEventsReport {
            retention_days: DEFAULT_AUDIT_RETENTION_DAYS,
            cutoff: Utc::now(),
            dry_run: false,
            deletable_count: 0,
            deleted_count: 0,
            vacuum_ran: false,
        });
        assert_eq!(
            audit_prune_periodic_outcome(&ok),
            PeriodicOutcome::Completed
        );
    }

    #[test]
    fn audit_retention_cutoff_rejects_unrepresentable_day_count() {
        assert!(audit_retention_cutoff(u64::MAX).is_err());
    }
}

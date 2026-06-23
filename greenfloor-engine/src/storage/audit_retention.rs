//! Retention policy for append-only `audit_event` rows.

use crate::operator_log::{
    COIN_OPS_EXECUTED, COIN_OP_LEDGER_EXECUTED, OFFER_LIFECYCLE_TRANSITION, TAKER_DETECTION,
};

pub const DEFAULT_AUDIT_RETENTION_DAYS: u64 = 30;
pub const DEFAULT_AUDIT_PRUNE_INTERVAL_SECONDS: u64 = 86_400;

/// Event types that are always kept once written (no age cutoff).
pub const FINANCIALLY_IMPORTANT_AUDIT_EVENT_TYPES: &[&str] =
    &[TAKER_DETECTION, COIN_OP_LEDGER_EXECUTED, COIN_OPS_EXECUTED];

/// Offer lifecycle transitions that indicate on-chain coin movement from a taker.
pub const FINANCIALLY_IMPORTANT_OFFER_LIFECYCLE_STATES: &[&str] =
    &["mempool_observed", "tx_block_confirmed"];

/// SQL predicate matching financially important audit rows.
#[must_use]
pub fn financially_important_audit_predicate_sql() -> String {
    let event_types = FINANCIALLY_IMPORTANT_AUDIT_EVENT_TYPES
        .iter()
        .map(|event_type| format!("'{event_type}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let lifecycle_states = FINANCIALLY_IMPORTANT_OFFER_LIFECYCLE_STATES
        .iter()
        .map(|state| format!("'{state}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r"
        event_type IN ({event_types})
        OR (
            event_type = '{OFFER_LIFECYCLE_TRANSITION}'
            AND json_extract(payload_json, '$.new_state') IN ({lifecycle_states})
        )
        "
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn financially_important_predicate_includes_taker_and_coin_ops() {
        let sql = financially_important_audit_predicate_sql();
        assert!(sql.contains(TAKER_DETECTION));
        assert!(sql.contains(COIN_OP_LEDGER_EXECUTED));
        assert!(sql.contains(COIN_OPS_EXECUTED));
        assert!(sql.contains(OFFER_LIFECYCLE_TRANSITION));
        assert!(sql.contains("mempool_observed"));
        assert!(sql.contains("tx_block_confirmed"));
    }
}

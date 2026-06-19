use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::SignerResult;
use crate::operator_log::{
    DEXIE_OFFERS_ERROR, OFFER_CANCEL_POLICY, OFFER_LIFECYCLE_TRANSITION, OFFER_POST_FAILURE,
    OFFER_RECONCILIATION, STRATEGY_OFFER_EXECUTION, STRATEGY_OFFER_EXECUTION_ERROR,
    TAKER_DETECTION,
};
use crate::storage::{AuditEventRow, OfferStateListRow, SqliteStore};

const STATUS_EVENT_TYPES: &[&str] = &[
    STRATEGY_OFFER_EXECUTION,
    STRATEGY_OFFER_EXECUTION_ERROR,
    OFFER_POST_FAILURE,
    OFFER_CANCEL_POLICY,
    OFFER_LIFECYCLE_TRANSITION,
    TAKER_DETECTION,
    DEXIE_OFFERS_ERROR,
];

/// Legacy audit names — not emitted by current code; kept so `offers-status` can read old rows.
const LEGACY_STATUS_EVENT_TYPES: &[&str] = &[OFFER_RECONCILIATION];

fn status_event_types() -> Vec<&'static str> {
    STATUS_EVENT_TYPES
        .iter()
        .chain(LEGACY_STATUS_EVENT_TYPES.iter())
        .copied()
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferStatusRow {
    pub offer_id: String,
    pub market_id: String,
    pub state: String,
    pub last_seen_status: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferStatusAuditEvent {
    pub id: i64,
    pub event_type: String,
    pub market_id: Option<String>,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffersStatusCliResult {
    pub state_db: String,
    pub market_id: Option<String>,
    pub offer_count: u64,
    pub by_state: HashMap<String, u64>,
    pub offers: Vec<OfferStatusRow>,
    pub recent_events: Vec<OfferStatusAuditEvent>,
}

fn offer_status_row(row: OfferStateListRow) -> OfferStatusRow {
    OfferStatusRow {
        offer_id: row.offer_id,
        market_id: row.market_id,
        state: row.state,
        last_seen_status: row.last_seen_status,
        updated_at: row.updated_at,
    }
}

fn audit_event_row(row: AuditEventRow) -> OfferStatusAuditEvent {
    OfferStatusAuditEvent {
        id: row.id,
        event_type: row.event_type,
        market_id: row.market_id,
        payload: row.payload,
        created_at: row.created_at,
    }
}

/// Offers status cli.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn offers_status_cli(
    db_path: &Path,
    market_id: Option<&str>,
    limit: usize,
    events_limit: usize,
) -> SignerResult<OffersStatusCliResult> {
    let store = SqliteStore::open(db_path)?;
    let market_filter = market_id.map(str::trim).filter(|value| !value.is_empty());
    let offers = store
        .list_offer_states(market_filter, limit)?
        .into_iter()
        .map(offer_status_row)
        .collect::<Vec<_>>();
    let events = store
        .list_recent_audit_events(
            Some(status_event_types().as_slice()),
            market_filter,
            events_limit,
        )?
        .into_iter()
        .map(audit_event_row)
        .collect::<Vec<_>>();
    let mut by_state = HashMap::default();
    for row in &offers {
        *by_state.entry(row.state.clone()).or_insert(0) += 1;
    }
    Ok(OffersStatusCliResult {
        state_db: db_path.display().to_string(),
        market_id: market_filter.map(str::to_string),
        offer_count: crate::metrics::metric_collection_len_to_u64(offers.len()),
        by_state,
        offers,
        recent_events: events,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    use crate::operator_log::{audit_row, AuditDurability, LogContext};

    #[test]
    fn status_cli_reports_counts_and_events() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("a1", "m1", "open", Some(0))
            .expect("seed");
        store
            .upsert_offer_state("a2", "m1", "tx_block_confirmed", Some(4))
            .expect("seed");
        audit_row(
            &store,
            LogContext::DAEMON_CYCLE,
            OFFER_RECONCILIATION,
            &json!({"offer_id": "a2", "new_state": "tx_block_confirmed"}),
            Some("m1"),
            AuditDurability::Required,
        )
        .expect("audit");

        let payload = offers_status_cli(&db_path, Some("m1"), 20, 10).expect("status");
        assert_eq!(payload.offer_count, 2);
        assert_eq!(payload.by_state.get("open"), Some(&1));
        assert_eq!(payload.by_state.get("tx_block_confirmed"), Some(&1));
        assert_eq!(payload.recent_events.len(), 1);
    }
}

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::daemon::{cancel_offers_on_dexie, CancelOfferTarget};
use crate::adapters::DexieClient;
use crate::error::{SignerError, SignerResult};
use crate::storage::{AuditEventRow, OfferStateListRow, SqliteStore};

const STATUS_EVENT_TYPES: &[&str] = &[
    "strategy_offer_execution",
    "offer_cancel_policy",
    "offer_lifecycle_transition",
    "offer_reconciliation",
    "taker_detection",
    "dexie_offers_error",
];

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffersCancelCliItem {
    pub offer_id: String,
    pub market_id: String,
    pub state: String,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffersCancelCliResult {
    pub venue: String,
    pub cancel_open: bool,
    pub requested_offer_ids: Vec<String>,
    pub selected_count: u64,
    pub cancelled_count: u64,
    pub failed_count: u64,
    pub items: Vec<OffersCancelCliItem>,
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
            Some(STATUS_EVENT_TYPES),
            market_filter,
            events_limit,
        )?
        .into_iter()
        .map(audit_event_row)
        .collect::<Vec<_>>();
    let mut by_state = HashMap::new();
    for row in &offers {
        *by_state.entry(row.state.clone()).or_insert(0) += 1;
    }
    Ok(OffersStatusCliResult {
        state_db: db_path.display().to_string(),
        market_id: market_filter.map(str::to_string),
        offer_count: offers.len() as u64,
        by_state,
        offers,
        recent_events: events,
    })
}

#[derive(Debug, Clone)]
struct SelectedOffer {
    offer_id: String,
    market_id: String,
    state: String,
}

fn select_offers_for_cancel(
    rows: &[OfferStateListRow],
    offer_ids: &[String],
    cancel_open: bool,
) -> SignerResult<Vec<SelectedOffer>> {
    let normalized = rows
        .iter()
        .filter_map(|row| {
            let offer_id = row.offer_id.trim();
            if offer_id.is_empty() {
                return None;
            }
            Some(SelectedOffer {
                offer_id: offer_id.to_string(),
                market_id: row.market_id.trim().to_string(),
                state: row.state.trim().to_ascii_lowercase(),
            })
        })
        .collect::<Vec<_>>();
    if cancel_open {
        return Ok(normalized
            .into_iter()
            .filter(|row| row.state == "open")
            .collect());
    }
    let requested_ids: HashSet<String> = offer_ids
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if requested_ids.is_empty() {
        return Err(SignerError::Other(
            "provide at least one --offer-id or pass --cancel-open".to_string(),
        ));
    }
    Ok(normalized
        .into_iter()
        .filter(|row| requested_ids.contains(&row.offer_id))
        .collect())
}

pub async fn offers_cancel_cli(
    db_path: &Path,
    dexie_base_url: &str,
    target_venue: &str,
    offer_ids: &[String],
    cancel_open: bool,
) -> SignerResult<OffersCancelCliResult> {
    let venue = target_venue.trim().to_ascii_lowercase();
    if venue != "dexie" {
        return Err(SignerError::Other(format!(
            "offer cancel supports dexie venue only (got {venue})"
        )));
    }
    let store = SqliteStore::open(db_path)?;
    let dexie = DexieClient::new(dexie_base_url);
    let requested_offer_ids: Vec<String> = offer_ids
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    let rows = store.list_offer_states(None, 500)?;
    let selected = select_offers_for_cancel(&rows, &requested_offer_ids, cancel_open)?;
    let targets: Vec<CancelOfferTarget> = selected
        .iter()
        .map(|row| CancelOfferTarget {
            offer_id: row.offer_id.clone(),
            market_id: row.market_id.clone(),
        })
        .collect();
    let outcomes = cancel_offers_on_dexie(&store, &dexie, &targets).await?;
    let mut items = Vec::with_capacity(outcomes.len());
    let mut failures = 0u64;
    for (outcome, row) in outcomes.into_iter().zip(selected.into_iter()) {
        if !outcome.success {
            failures += 1;
        }
        items.push(OffersCancelCliItem {
            offer_id: row.offer_id,
            market_id: row.market_id,
            state: row.state,
            result: json!({
                "success": outcome.success,
                "venue_response": outcome.venue_response,
                "error": outcome.error,
            }),
        });
    }
    let selected_count = items.len() as u64;
    Ok(OffersCancelCliResult {
        venue,
        cancel_open,
        requested_offer_ids,
        selected_count,
        cancelled_count: selected_count.saturating_sub(failures),
        failed_count: failures,
        items,
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

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
        store
            .add_audit_event(
                "offer_reconciliation",
                &json!({"offer_id": "a2", "new_state": "tx_block_confirmed"}),
                Some("m1"),
            )
            .expect("audit");

        let payload = offers_status_cli(&db_path, Some("m1"), 20, 10).expect("status");
        assert_eq!(payload.offer_count, 2);
        assert_eq!(payload.by_state.get("open"), Some(&1));
        assert_eq!(payload.by_state.get("tx_block_confirmed"), Some(&1));
        assert_eq!(payload.recent_events.len(), 1);
    }

    #[tokio::test]
    async fn cancel_cli_cancel_open_updates_state() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-open", "m1", "open", Some(0))
            .expect("seed");
        store
            .upsert_offer_state("offer-expired", "m1", "expired", Some(0))
            .expect("seed");

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/v1/offers/offer-open/cancel")
            .with_status(200)
            .with_body(r#"{"success":true,"id":"offer-open","status":3}"#)
            .create();

        let payload = offers_cancel_cli(
            &db_path,
            &server.url(),
            "dexie",
            &[],
            true,
        )
        .await
        .expect("cancel");
        assert_eq!(payload.selected_count, 1);
        assert_eq!(payload.cancelled_count, 1);
        assert_eq!(payload.failed_count, 0);
        assert_eq!(payload.items[0].offer_id, "offer-open");

        let rows = store
            .list_offer_states(None, 10)
            .expect("rows");
        let by_id: HashMap<_, _> = rows
            .into_iter()
            .map(|row| (row.offer_id, row.state))
            .collect();
        assert_eq!(by_id.get("offer-open").map(String::as_str), Some("cancelled"));
        assert_eq!(by_id.get("offer-expired").map(String::as_str), Some("expired"));
    }

    #[tokio::test]
    async fn cancel_cli_reports_dexie_failure() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-fail", "m1", "open", Some(0))
            .expect("seed");

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/v1/offers/offer-fail/cancel")
            .with_status(200)
            .with_body(r#"{"success":false,"error":"not_found"}"#)
            .create();

        let payload = offers_cancel_cli(
            &db_path,
            &server.url(),
            "dexie",
            &["offer-fail".to_string()],
            false,
        )
        .await
        .expect("cancel");
        assert_eq!(payload.cancelled_count, 0);
        assert_eq!(payload.failed_count, 1);
        assert_eq!(
            payload.items[0]
                .result
                .get("error")
                .and_then(Value::as_str),
            Some("not_found")
        );
    }
}

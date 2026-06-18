use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::adapters::DexieClient;
use crate::error::{SignerError, SignerResult};
use crate::storage::{OfferStateListRow, SqliteStore};

use super::cancel::{cancel_offers_on_dexie, CancelOfferTarget};

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
    for (outcome, row) in outcomes.into_iter().zip(selected) {
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
    let selected_count = crate::metrics::metric_collection_len_to_u64(items.len());
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
    use std::collections::HashMap;

    use tempfile::tempdir;

    use super::*;

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

        let payload = offers_cancel_cli(&db_path, &server.url(), "dexie", &[], true)
            .await
            .expect("cancel");
        assert_eq!(payload.selected_count, 1);
        assert_eq!(payload.cancelled_count, 1);
        assert_eq!(payload.failed_count, 0);
        assert_eq!(payload.items[0].offer_id, "offer-open");

        let rows = store.list_offer_states(None, 10).expect("rows");
        let by_id: HashMap<_, _> = rows
            .into_iter()
            .map(|row| (row.offer_id, row.state))
            .collect();
        assert_eq!(
            by_id.get("offer-open").map(String::as_str),
            Some("cancelled")
        );
        assert_eq!(
            by_id.get("offer-expired").map(String::as_str),
            Some("expired")
        );
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
            payload.items[0].result.get("error").and_then(Value::as_str),
            Some("not_found")
        );
    }
}

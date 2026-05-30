use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::DexieClient;
use crate::cycle::{
    is_dexie_offer_missing_error_text, resolve_missing_watched_offer_transition,
    unchanged_offer_transition, unsupported_venue_offer_transition, CycleOfferTransition,
};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::reconcile_augment::missing_offer_error_from_payload;
use super::reconcile_phase::transition_from_dexie_offer_payload;
use super::reconcile_persist::{persist_offer_lifecycle_transition, ReconcilePersistOptions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileBatchItem {
    pub offer_id: String,
    pub market_id: String,
    pub old_state: String,
    pub new_state: String,
    pub changed: bool,
    pub last_seen_status: Option<i64>,
    pub reason: String,
    pub taker_signal: String,
    pub taker_diagnostic: String,
    pub signal_source: String,
    pub coinset_tx_ids: Vec<String>,
    pub coinset_confirmed_tx_ids: Vec<String>,
    pub coinset_mempool_tx_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileBatchResult {
    pub items: Vec<ReconcileBatchItem>,
    pub reconciled_count: u64,
    pub changed_count: u64,
}

fn batch_item_from_transition(
    offer_id: &str,
    market_id: &str,
    transition: &CycleOfferTransition,
    last_seen_status: Option<i64>,
) -> ReconcileBatchItem {
    ReconcileBatchItem {
        offer_id: offer_id.to_string(),
        market_id: market_id.to_string(),
        old_state: transition.old_state.clone(),
        new_state: transition.new_state.clone(),
        changed: transition.changed,
        last_seen_status,
        reason: transition.reason.clone(),
        taker_signal: transition.taker_signal.clone(),
        taker_diagnostic: transition.taker_diagnostic.clone(),
        signal_source: transition.signal_source.clone(),
        coinset_tx_ids: transition.coinset_tx_ids.clone(),
        coinset_confirmed_tx_ids: transition.coinset_confirmed_tx_ids.clone(),
        coinset_mempool_tx_ids: transition.coinset_mempool_tx_ids.clone(),
    }
}

async fn reconcile_offer_row(
    store: &SqliteStore,
    dexie: &DexieClient,
    target_venue: &str,
    offer_id: &str,
    market_id: &str,
    current_state: &str,
) -> SignerResult<(CycleOfferTransition, Option<i64>)> {
    if target_venue != "dexie" {
        let transition =
            unsupported_venue_offer_transition(current_state, target_venue).map_err(|err| {
                crate::error::SignerError::Other(err.to_string())
            })?;
        return Ok((transition, None));
    }

    match dexie.get_offer(offer_id).await {
        Ok(payload) => {
            if missing_offer_error_from_payload(&payload).is_some() {
                let transition = resolve_missing_watched_offer_transition(current_state)
                    .map_err(|err| crate::error::SignerError::Other(err.to_string()))?;
                return Ok((transition, None));
            }
            let offer_body = payload.get("offer").unwrap_or(&payload);
            let status = super::coinset_tx::dexie_offer_status(offer_body);
            let transition =
                transition_from_dexie_offer_payload(store, current_state, offer_body)?;
            Ok((transition, status))
        }
        Err(err) if is_dexie_offer_missing_error_text(&err.to_string()) => {
            let transition = resolve_missing_watched_offer_transition(current_state)
                .map_err(|err| crate::error::SignerError::Other(err.to_string()))?;
            Ok((transition, None))
        }
        Err(err) => {
            let transition = unchanged_offer_transition(
                current_state,
                &format!("dexie_lookup_error:{err}"),
            )
            .map_err(|parse_err| crate::error::SignerError::Other(parse_err.to_string()))?;
            Ok((transition, None))
        }
    }
}

pub async fn reconcile_offers_batch(
    db_path: &Path,
    dexie_base_url: &str,
    target_venue: &str,
    market_id: Option<&str>,
    limit: usize,
) -> SignerResult<ReconcileBatchResult> {
    let store = SqliteStore::open(db_path)?;
    let venue = target_venue.trim().to_ascii_lowercase();
    let dexie = if venue == "dexie" {
        Some(DexieClient::new(dexie_base_url))
    } else {
        None
    };

    let rows = store.list_offer_states(market_id, limit)?;
    let mut items = Vec::with_capacity(rows.len());
    let mut changed_count = 0u64;

    for row in rows {
        let (transition, last_seen_status) = if let Some(dexie) = dexie.as_ref() {
            reconcile_offer_row(
                &store,
                dexie,
                &venue,
                &row.offer_id,
                &row.market_id,
                &row.state,
            )
            .await?
        } else {
            let transition =
                unsupported_venue_offer_transition(&row.state, &venue).map_err(|err| {
                    crate::error::SignerError::Other(err.to_string())
                })?;
            (transition, None)
        };

        persist_offer_lifecycle_transition(
            &store,
            &row.market_id,
            &row.offer_id,
            &transition,
            last_seen_status,
            ReconcilePersistOptions {
                action: "offers_reconcile",
                venue: Some(&venue),
                dexie_error: None,
            },
        )?;

        if transition.changed {
            changed_count += 1;
        }
        items.push(batch_item_from_transition(
            &row.offer_id,
            &row.market_id,
            &transition,
            last_seen_status,
        ));
    }

    Ok(ReconcileBatchResult {
        reconciled_count: items.len() as u64,
        changed_count,
        items,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::adapters::DexieClient;

    #[tokio::test]
    async fn batch_reconcile_updates_states_from_dexie() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        let confirmed_tx_id = "a".repeat(64);
        store
            .upsert_offer_state("offer-ok", "m1", "open", Some(0))
            .expect("seed");
        store
            .upsert_offer_state("offer-missing", "m1", "open", Some(0))
            .expect("seed");
        assert_eq!(store.observe_mempool_tx_ids(&[confirmed_tx_id.clone()]).expect("mempool"), 1);
        assert_eq!(
            store.confirm_tx_ids(&[confirmed_tx_id.clone()]).expect("confirm"),
            1
        );

        let mut server = mockito::Server::new_async().await;
        let _ok = server
            .mock("GET", "/v1/offers/offer-ok")
            .with_status(200)
            .with_body(
                json!({"id":"offer-ok","status":4,"tx_id":confirmed_tx_id}).to_string(),
            )
            .create();
        let _missing = server
            .mock("GET", "/v1/offers/offer-missing")
            .with_status(404)
            .with_body(r#"{"success":false,"error":"not_found"}"#)
            .create();

        let batch = reconcile_offers_batch(
            &db_path,
            &server.url(),
            "dexie",
            None,
            20,
        )
        .await
        .expect("batch");

        assert_eq!(batch.reconciled_count, 2);
        assert_eq!(batch.changed_count, 2);
        let rows = store
            .list_offer_state_details("m1", 20)
            .expect("rows");
        let by_id: std::collections::HashMap<_, _> = rows
            .into_iter()
            .map(|row| (row.offer_id, row.state))
            .collect();
        assert_eq!(by_id.get("offer-ok").map(String::as_str), Some("tx_block_confirmed"));
        assert_eq!(by_id.get("offer-missing").map(String::as_str), Some("expired"));
    }
}

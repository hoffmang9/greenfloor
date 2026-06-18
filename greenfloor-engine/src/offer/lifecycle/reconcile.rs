use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::adapters::DexieClient;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::persist::{persist_offer_lifecycle_transition, ReconcilePersistOptions};
use super::transition::resolve_watched_offer_transition_for_venue;

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
    transition: &crate::cycle::CycleOfferTransition,
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
        let (transition, last_seen_status, _dexie_error) =
            resolve_watched_offer_transition_for_venue(
                &store,
                dexie.as_ref(),
                &venue,
                &row.offer_id,
                &row.state,
            )
            .await?;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileCliResult {
    pub state_db: String,
    pub venue: String,
    pub market_id: Option<String>,
    pub reconciled_count: u64,
    pub changed_count: u64,
    pub items: Vec<ReconcileBatchItem>,
}

pub async fn reconcile_offers_cli(
    db_path: &Path,
    dexie_base_url: &str,
    target_venue: &str,
    market_id: Option<&str>,
    limit: usize,
) -> SignerResult<ReconcileCliResult> {
    let batch =
        reconcile_offers_batch(db_path, dexie_base_url, target_venue, market_id, limit).await?;
    Ok(ReconcileCliResult {
        state_db: db_path.display().to_string(),
        venue: target_venue.trim().to_ascii_lowercase(),
        market_id: market_id.map(str::to_string),
        reconciled_count: batch.reconciled_count,
        changed_count: batch.changed_count,
        items: batch.items,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

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
        assert_eq!(
            store
                .observe_mempool_tx_ids(&[confirmed_tx_id.clone()])
                .expect("mempool"),
            1
        );
        assert_eq!(
            store
                .confirm_tx_ids(&[confirmed_tx_id.clone()])
                .expect("confirm"),
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
        assert_eq!(
            by_id.get("offer-ok").map(String::as_str),
            Some("tx_block_confirmed")
        );
        assert_eq!(
            by_id.get("offer-missing").map(String::as_str),
            Some("expired")
        );
    }
}

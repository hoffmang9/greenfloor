use crate::adapters::DexieClient;
use crate::cycle::{
    classify_dexie_stale_offer_status, collect_stale_sweep_candidates,
    is_dexie_offer_missing_error_text, record_stale_sweep_check, OfferStateRow, StaleSweepHit,
    StaleSweepProgress,
};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

const GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET: usize = 3;
const GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS: usize = 60;

pub async fn detect_stale_open_offers_for_requeue(
    store: &SqliteStore,
    dexie: &DexieClient,
    enabled_market_ids: &[String],
) -> SignerResult<StaleSweepProgress> {
    if enabled_market_ids.is_empty() {
        return Ok(StaleSweepProgress {
            checked_offer_count: 0,
            requeue_market_ids: Vec::new(),
            hits: Vec::new(),
            truncated: false,
        });
    }

    let rows = store.list_offer_states(None, 5000)?;
    let offer_rows: Vec<OfferStateRow> = rows
        .into_iter()
        .map(|row| OfferStateRow {
            market_id: row.market_id,
            offer_id: row.offer_id,
            state: row.state,
        })
        .collect();
    let enabled_set: Vec<String> = enabled_market_ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    let candidates = collect_stale_sweep_candidates(
        &offer_rows,
        &enabled_set,
        GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET,
    );

    let mut progress = StaleSweepProgress {
        checked_offer_count: 0,
        requeue_market_ids: Vec::new(),
        hits: Vec::new(),
        truncated: false,
    };
    let check_limit = GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS.max(1);

    for candidate in candidates {
        if progress.checked_offer_count >= check_limit {
            return Ok(StaleSweepProgress {
                truncated: true,
                ..progress
            });
        }
        let market_id = candidate.market_id.trim().to_string();
        let offer_id = candidate.offer_id.trim().to_string();
        let hit = match dexie.get_offer(&offer_id).await {
            Ok(response) => {
                if response.is_explicit_failure() {
                    if is_dexie_offer_missing_error_text(response.error_text()) {
                        Some(StaleSweepHit {
                            market_id: market_id.clone(),
                            offer_id: offer_id.clone(),
                            reason: "offer_missing_404".to_string(),
                        })
                    } else {
                        None
                    }
                } else if let Some(offer_obj) = response.offer_payload() {
                    let status = offer_obj
                        .get("status")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(-1);
                    classify_dexie_stale_offer_status(status).map(|reason| StaleSweepHit {
                        market_id: market_id.clone(),
                        offer_id: offer_id.clone(),
                        reason: reason.to_string(),
                    })
                } else {
                    None
                }
            }
            Err(err) if is_dexie_offer_missing_error_text(&err.to_string()) => {
                Some(StaleSweepHit {
                    market_id: market_id.clone(),
                    offer_id: offer_id.clone(),
                    reason: "offer_missing_404".to_string(),
                })
            }
            Err(_) => None,
        };
        progress = record_stale_sweep_check(&progress, hit);
    }

    Ok(progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use tempfile::tempdir;

    #[tokio::test]
    async fn detect_stale_open_offers_marks_expired_status() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-expired", "m1", "open", Some(0))
            .expect("seed");

        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/offers/offer-expired")
            .with_status(200)
            .with_body(r#"{"success":true,"offer":{"id":"offer-expired","status":6}}"#)
            .create();
        let dexie = DexieClient::new(server.url());

        let progress = detect_stale_open_offers_for_requeue(&store, &dexie, &["m1".to_string()])
            .await
            .expect("sweep");

        assert_eq!(progress.checked_offer_count, 1);
        assert_eq!(progress.requeue_market_ids, vec!["m1".to_string()]);
        assert_eq!(progress.hits[0].reason, "offer_expired");
    }

    #[tokio::test]
    async fn detect_stale_open_offers_marks_missing_404() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-missing", "m2", "open", Some(0))
            .expect("seed");

        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/offers/offer-missing")
            .with_status(404)
            .with_body(r#"{"success":false,"error":"not found"}"#)
            .create();
        let dexie = DexieClient::new(server.url());

        let progress = detect_stale_open_offers_for_requeue(&store, &dexie, &["m2".to_string()])
            .await
            .expect("sweep");

        assert_eq!(progress.checked_offer_count, 1);
        assert_eq!(progress.requeue_market_ids, vec!["m2".to_string()]);
        assert_eq!(progress.hits[0].reason, "offer_missing_404");
    }
}

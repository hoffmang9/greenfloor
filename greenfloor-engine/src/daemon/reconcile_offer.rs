//! Canonical watched-offer reconcile: Dexie lookup/transition resolution shared by
//! daemon cycle reconcile, CLI batch reconcile, and watchlist augment.

use serde_json::Value;

use crate::adapters::DexieClient;
use crate::cycle::{
    is_dexie_offer_missing_error_text, resolve_missing_watched_offer_transition,
    unchanged_offer_transition, unsupported_venue_offer_transition, CycleOfferTransition,
};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::coinset_tx::dexie_offer_status;
use super::dexie_offer::DexieOfferPayload;
use super::reconcile_phase::transition_from_dexie_offer_payload;

pub(crate) fn missing_offer_error_from_payload(payload: &Value) -> Option<String> {
    if payload.get("success") != Some(&Value::Bool(false)) {
        return None;
    }
    let error_text = payload
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("");
    if is_dexie_offer_missing_error_text(error_text) {
        Some(error_text.to_string())
    } else {
        None
    }
}

fn missing_watched_offer_transition(current_state: &str) -> SignerResult<CycleOfferTransition> {
    resolve_missing_watched_offer_transition(current_state)
        .map_err(|err| crate::error::SignerError::Other(err.to_string()))
}

fn transition_from_offer_body(
    store: &SqliteStore,
    current_state: &str,
    offer_body: &Value,
) -> SignerResult<(CycleOfferTransition, Option<i64>)> {
    let status = dexie_offer_status(offer_body);
    let transition = transition_from_dexie_offer_payload(store, current_state, offer_body)?;
    Ok((transition, status))
}

/// Resolve a lifecycle transition from an already-fetched Dexie offer payload.
pub(crate) fn transition_from_list_offer_payload(
    store: &SqliteStore,
    current_state: &str,
    offer_payload: &Value,
) -> SignerResult<(CycleOfferTransition, Option<i64>)> {
    let offer = DexieOfferPayload::new(offer_payload.clone());
    transition_from_offer_body(store, current_state, offer.body())
}

/// Resolve a lifecycle transition by fetching a single offer from Dexie.
pub async fn resolve_watched_offer_transition_from_dexie_fetch(
    store: &SqliteStore,
    dexie: &DexieClient,
    offer_id: &str,
    current_state: &str,
) -> SignerResult<(CycleOfferTransition, Option<i64>, Option<String>)> {
    match dexie.get_offer(offer_id).await {
        Ok(payload) => {
            if let Some(error_text) = missing_offer_error_from_payload(&payload) {
                let transition = missing_watched_offer_transition(current_state)?;
                return Ok((transition, None, Some(error_text)));
            }
            let offer_body = payload.get("offer").unwrap_or(&payload);
            let (transition, status) =
                transition_from_offer_body(store, current_state, offer_body)?;
            Ok((transition, status, None))
        }
        Err(err) if is_dexie_offer_missing_error_text(&err.to_string()) => {
            let transition = missing_watched_offer_transition(current_state)?;
            Ok((transition, None, Some(err.to_string())))
        }
        Err(err) => {
            let transition = unchanged_offer_transition(
                current_state,
                &format!("dexie_lookup_error:{err}"),
            )
            .map_err(|parse_err| crate::error::SignerError::Other(parse_err.to_string()))?;
            Ok((transition, None, None))
        }
    }
}

pub async fn resolve_watched_offer_transition_for_venue(
    store: &SqliteStore,
    dexie: Option<&DexieClient>,
    target_venue: &str,
    offer_id: &str,
    current_state: &str,
) -> SignerResult<(CycleOfferTransition, Option<i64>, Option<String>)> {
    if target_venue != "dexie" {
        let transition =
            unsupported_venue_offer_transition(current_state, target_venue).map_err(|err| {
                crate::error::SignerError::Other(err.to_string())
            })?;
        return Ok((transition, None, None));
    }
    let Some(dexie) = dexie else {
        return Err(crate::error::SignerError::Other(
            "dexie client required for dexie venue reconcile".to_string(),
        ));
    };
    resolve_watched_offer_transition_from_dexie_fetch(store, dexie, offer_id, current_state).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::adapters::DexieClient;
    use crate::storage::SqliteStore;

    #[tokio::test]
    async fn fetch_transition_expires_on_dexie_404() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/offers/offer-missing")
            .with_status(404)
            .with_body(r#"{"success":false,"error":"not_found"}"#)
            .create();
        let dexie = DexieClient::new(server.url());
        let (transition, status, error) = resolve_watched_offer_transition_from_dexie_fetch(
            &store,
            &dexie,
            "offer-missing",
            "open",
        )
        .await
        .expect("transition");
        assert_eq!(transition.new_state, "expired");
        assert!(status.is_none());
        assert!(error.is_some());
    }
}

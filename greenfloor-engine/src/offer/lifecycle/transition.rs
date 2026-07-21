//! Canonical watched-offer reconcile: Dexie lookup/transition resolution shared by
//! daemon cycle reconcile, CLI batch reconcile, and watchlist augment.

use std::collections::HashMap;

use serde_json::Value;

use chrono::{DateTime, Utc};

use crate::adapters::DexieClient;
use crate::cycle::reconcile::CancelSubmittedContext;
use crate::cycle::{
    is_dexie_offer_missing_error_text, resolve_missing_watched_offer_transition,
    resolve_watched_offer_transition_from_signals, unchanged_offer_transition,
    unsupported_venue_offer_transition, CycleOfferTransition,
};
use crate::error::SignerResult;
use crate::offer::dexie_payload::{dexie_offer_status, extract_coinset_tx_ids_from_offer_payload};
use crate::storage::SqliteStore;

use super::cancel_context::{
    cancel_submitted_context_for_offer, chain_confirmed_tx_ids_for_transition,
};
use crate::cycle::reconcile::CoinsetTxSignals;

/// Clock and optional preloaded cancel-submit context for watched-offer reconcile.
#[derive(Debug, Clone, Copy)]
pub struct WatchedOfferTransitionEnv<'a> {
    pub now: DateTime<Utc>,
    pub cancel_submitted_by_offer: Option<&'a HashMap<String, CancelSubmittedContext>>,
}

impl<'a> WatchedOfferTransitionEnv<'a> {
    #[must_use]
    pub fn new(
        now: DateTime<Utc>,
        cancel_submitted_by_offer: Option<&'a HashMap<String, CancelSubmittedContext>>,
    ) -> Self {
        Self {
            now,
            cancel_submitted_by_offer,
        }
    }

    #[must_use]
    pub fn at_now(
        cancel_submitted_by_offer: Option<&'a HashMap<String, CancelSubmittedContext>>,
    ) -> Self {
        Self::new(Utc::now(), cancel_submitted_by_offer)
    }
}

fn coinset_signal_lists(
    store: &SqliteStore,
    coinset_tx_ids: &[String],
) -> SignerResult<(Vec<String>, Vec<String>)> {
    if coinset_tx_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let signal_by_tx_id = store.get_tx_signal_state(coinset_tx_ids)?;
    let mut confirmed = Vec::new();
    let mut mempool = Vec::new();
    for tx_id in coinset_tx_ids {
        let Some(signal) = signal_by_tx_id.get(tx_id) else {
            continue;
        };
        if signal.tx_block_confirmed_at.is_some() {
            confirmed.push(tx_id.clone());
            continue;
        }
        if signal.mempool_observed_at.is_some() {
            mempool.push(tx_id.clone());
        }
    }
    Ok((confirmed, mempool))
}

/// Dexie list/get status + Coinset tx signal lists for the shared apply spine.
///
/// # Errors
///
/// Returns an error if `SQLite` tx-signal reads fail.
pub fn coinset_signals_from_dexie_offer_payload(
    store: &SqliteStore,
    offer_payload: &Value,
) -> SignerResult<(Option<i64>, CoinsetTxSignals)> {
    let status = dexie_offer_status(offer_payload);
    let coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(offer_payload);
    let (confirmed_tx_ids, mempool_tx_ids) = coinset_signal_lists(store, &coinset_tx_ids)?;
    Ok((
        status,
        CoinsetTxSignals {
            tx_ids: coinset_tx_ids,
            confirmed_tx_ids,
            mempool_tx_ids,
            ..Default::default()
        },
    ))
}

/// Transition from dexie offer payload.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn transition_from_dexie_offer_payload(
    store: &SqliteStore,
    offer_id: &str,
    current_state: &str,
    offer_payload: &Value,
    env: WatchedOfferTransitionEnv<'_>,
) -> SignerResult<CycleOfferTransition> {
    let (status, signals) = coinset_signals_from_dexie_offer_payload(store, offer_payload)?;
    let cancel_submitted = cancel_submitted_context_for_offer(
        store,
        offer_id,
        current_state,
        env.cancel_submitted_by_offer,
    )?;
    let chain_confirmed_tx_ids = chain_confirmed_tx_ids_for_transition(
        store,
        cancel_submitted.as_ref(),
        &signals.confirmed_tx_ids,
    )?;
    resolve_watched_offer_transition_from_signals(
        current_state,
        status,
        signals,
        &chain_confirmed_tx_ids,
        cancel_submitted.as_ref(),
        env.now,
    )
    .map_err(|err| crate::error::SignerError::Other(err.to_string()))
}

pub fn missing_offer_error_from_payload(payload: &Value) -> Option<String> {
    if payload.get("success") != Some(&Value::Bool(false)) {
        return None;
    }
    let error_text = payload.get("error").and_then(Value::as_str).unwrap_or("");
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
    offer_id: &str,
    current_state: &str,
    offer_body: &Value,
    env: WatchedOfferTransitionEnv<'_>,
) -> SignerResult<(CycleOfferTransition, Option<i64>)> {
    let status = dexie_offer_status(offer_body);
    let transition =
        transition_from_dexie_offer_payload(store, offer_id, current_state, offer_body, env)?;
    Ok((transition, status))
}

/// Resolve a lifecycle transition by fetching a single offer from Dexie.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn resolve_watched_offer_transition_from_dexie_fetch(
    store: &SqliteStore,
    dexie: &DexieClient,
    offer_id: &str,
    current_state: &str,
    env: WatchedOfferTransitionEnv<'_>,
) -> SignerResult<(CycleOfferTransition, Option<i64>, Option<String>)> {
    match dexie.get_offer(offer_id).await {
        Ok(response) => {
            let payload = response.body();
            if let Some(error_text) = missing_offer_error_from_payload(payload) {
                let transition = missing_watched_offer_transition(current_state)?;
                return Ok((transition, None, Some(error_text)));
            }
            let offer_body = payload.get("offer").unwrap_or(payload);
            let (transition, status) =
                transition_from_offer_body(store, offer_id, current_state, offer_body, env)?;
            Ok((transition, status, None))
        }
        Err(err) if is_dexie_offer_missing_error_text(&err.to_string()) => {
            let transition = missing_watched_offer_transition(current_state)?;
            Ok((transition, None, Some(err.to_string())))
        }
        Err(err) => {
            let transition =
                unchanged_offer_transition(current_state, format!("dexie_lookup_error:{err}"))
                    .map_err(|parse_err| crate::error::SignerError::Other(parse_err.to_string()))?;
            Ok((transition, None, None))
        }
    }
}

/// Resolve watched offer transition for venue.
///
/// HTTP reconcile is Dexie-only. Coinset/splash lifecycle is driven by Coinset WS +
/// local watches — callers must filter those rows out before invoking this.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn resolve_watched_offer_transition_for_venue(
    store: &SqliteStore,
    dexie: Option<&DexieClient>,
    target_venue: crate::config::Venue,
    offer_id: &str,
    current_state: &str,
    env: WatchedOfferTransitionEnv<'_>,
) -> SignerResult<(CycleOfferTransition, Option<i64>, Option<String>)> {
    if !target_venue.is_dexie() {
        let transition = unsupported_venue_offer_transition(current_state, target_venue.as_str())
            .map_err(|err| crate::error::SignerError::Other(err.to_string()))?;
        return Ok((transition, None, None));
    }
    let Some(dexie) = dexie else {
        return Err(crate::error::SignerError::Other(
            "dexie client required for dexie venue reconcile".to_string(),
        ));
    };
    resolve_watched_offer_transition_from_dexie_fetch(store, dexie, offer_id, current_state, env)
        .await
}

#[cfg(test)]
mod tests {
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
            WatchedOfferTransitionEnv::at_now(None),
        )
        .await
        .expect("transition");
        assert_eq!(
            transition.new_state,
            crate::cycle::ReconcileState::parse("expired").expect("state")
        );
        assert!(status.is_none());
        assert!(error.is_some());
    }

    #[tokio::test]
    async fn non_dexie_venue_is_unsupported_for_http_reconcile() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let (transition, status, error) = resolve_watched_offer_transition_for_venue(
            &store,
            None,
            crate::config::Venue::Coinset,
            &"ab".repeat(32),
            "open",
            WatchedOfferTransitionEnv::at_now(None),
        )
        .await
        .expect("transition");
        assert_eq!(transition.new_state.as_str(), "reconcile_unsupported_venue");
        assert!(status.is_none());
        assert!(error.is_none());
    }
}

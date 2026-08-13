//! Canonical watched-offer reconcile: Dexie lookup/transition resolution shared by
//! daemon cycle reconcile, CLI batch reconcile, and watchlist augment.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::cycle::reconcile::{CancelSubmittedContext, CoinsetTxSignals};
use crate::cycle::{resolve_watched_offer_transition_from_signals, CycleOfferTransition};
use crate::error::SignerResult;
use crate::offer::dexie_payload::{dexie_offer_status, extract_coinset_tx_ids_from_offer_payload};
use crate::storage::SqliteStore;

use super::cancel_context::{
    cancel_submitted_context_for_offer, chain_confirmed_tx_ids_for_transition,
};

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

/// Resolve lifecycle + Dexie status from an offer body (list row or `get_offer.offer`).
///
/// # Errors
///
/// Returns an error if signal extraction or transition resolve fails.
pub(crate) fn transition_from_offer_body(
    store: &SqliteStore,
    offer_id: &str,
    current_state: &str,
    offer_body: &Value,
    env: WatchedOfferTransitionEnv<'_>,
) -> SignerResult<(CycleOfferTransition, Option<i64>)> {
    let (status, signals) = coinset_signals_from_dexie_offer_payload(store, offer_body)?;
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
    let transition = resolve_watched_offer_transition_from_signals(
        current_state,
        status,
        signals,
        &chain_confirmed_tx_ids,
        cancel_submitted.as_ref(),
        env.now,
    )?;
    Ok((transition, status))
}

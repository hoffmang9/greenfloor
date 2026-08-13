//! Shared resolve+persist for watched-offer Coinset, Dexie, and cancel-submitted signals.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::adapters::DexieClient;
use crate::cycle::reconcile::{
    resolve_watched_offer_transition_from_signals, CancelSubmittedContext, CoinsetTxSignals,
};
use crate::cycle::{resolve_missing_watched_offer_transition, CycleOfferTransition};
use crate::error::SignerResult;
use crate::storage::{OfferStateListRow, SqliteStore};

use super::cancel_context::{
    cancel_submitted_context_for_offer, chain_confirmed_tx_ids_for_transition,
    preload_cancel_submitted_contexts,
};
use super::persist::{persist_offer_lifecycle_transition, ReconcilePersistOptions};
use super::transition::{
    resolve_watched_offer_transition_from_dexie_fetch, transition_from_offer_body,
    WatchedOfferTransitionEnv,
};

fn persist_resolved_watched_transition(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    transition: &CycleOfferTransition,
    last_seen_status: Option<i64>,
    options: &ReconcilePersistOptions<'_>,
) -> SignerResult<()> {
    if transition.changed || last_seen_status.is_some() {
        persist_offer_lifecycle_transition(
            store,
            market_id,
            offer_id,
            transition,
            last_seen_status,
            options,
        )?;
    }
    Ok(())
}

/// Owns watched-offer signal ingestion, transition resolve, and persist (CLI, daemon, WS).
pub struct WatchedOfferReconciler<'a> {
    store: &'a SqliteStore,
    options: &'a ReconcilePersistOptions<'a>,
}

impl<'a> WatchedOfferReconciler<'a> {
    #[must_use]
    pub fn new(store: &'a SqliteStore, options: &'a ReconcilePersistOptions<'a>) -> Self {
        Self { store, options }
    }

    /// Apply Coinset signals to one persisted offer row.
    ///
    /// # Errors
    ///
    /// Returns an error if reconcile or persist fails.
    pub fn apply_row(
        &self,
        row: &OfferStateListRow,
        status: Option<i64>,
        signals: CoinsetTxSignals,
        cancel_by_offer: Option<&HashMap<String, CancelSubmittedContext>>,
        now: DateTime<Utc>,
    ) -> SignerResult<bool> {
        let cancel_submitted = cancel_submitted_context_for_offer(
            self.store,
            &row.offer_id,
            &row.state,
            cancel_by_offer,
        )?;
        let transition = self.apply_signals(
            &row.market_id,
            &row.offer_id,
            &row.state,
            status,
            signals,
            cancel_submitted.as_ref(),
            None,
            now,
        )?;
        Ok(transition.changed)
    }

    /// Apply empty-signal cancel-submitted policy to rows (orphan unwedge / cancel-tx promote).
    ///
    /// Past orphan grace, unconfirmed cancels reset to `open`. Within grace, non-attributable
    /// noise still preserves `cancel_submitted`. Callers that already ingested confirmed cancel
    /// txs rely on preloaded context seeing `tx_block_confirmed_at` for promotion.
    ///
    /// # Errors
    ///
    /// Returns an error if reconcile or persist fails.
    pub fn apply_cancel_submitted(
        &self,
        rows: &[OfferStateListRow],
        now: DateTime<Utc>,
    ) -> SignerResult<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let cancel_by_offer = preload_cancel_submitted_contexts(self.store, rows)?;
        let mut changed = 0_u64;
        for row in rows {
            if self.apply_row(
                row,
                None,
                CoinsetTxSignals::default(),
                Some(&cancel_by_offer),
                now,
            )? {
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Persist an already-resolved transition.
    ///
    /// # Errors
    ///
    /// Returns an error if persist fails.
    pub fn persist_transition(
        &self,
        market_id: &str,
        offer_id: &str,
        transition: &CycleOfferTransition,
        last_seen_status: Option<i64>,
    ) -> SignerResult<()> {
        persist_resolved_watched_transition(
            self.store,
            market_id,
            offer_id,
            transition,
            last_seen_status,
            self.options,
        )
    }

    /// Resolve + persist lifecycle from an already-fetched Dexie offer payload.
    ///
    /// # Errors
    ///
    /// Returns an error if signal extraction or `SQLite` persist fails.
    pub fn apply_dexie_payload(
        &self,
        market_id: &str,
        offer_id: &str,
        current_state: &str,
        offer_payload: &Value,
        env: WatchedOfferTransitionEnv<'_>,
    ) -> SignerResult<(CycleOfferTransition, Option<i64>)> {
        let (transition, status) =
            transition_from_offer_body(self.store, offer_id, current_state, offer_payload, env)?;
        self.persist_transition(market_id, offer_id, &transition, status)?;
        Ok((transition, status))
    }

    /// Resolve + persist a missing Dexie watched offer (404 / not-found).
    ///
    /// Callers put the Dexie error text on `options.dexie_error`.
    ///
    /// # Errors
    ///
    /// Returns an error if transition resolve or `SQLite` persist fails.
    pub fn apply_missing(
        &self,
        market_id: &str,
        offer_id: &str,
        current_state: &str,
    ) -> SignerResult<CycleOfferTransition> {
        let transition = resolve_missing_watched_offer_transition(current_state)?;
        self.persist_transition(market_id, offer_id, &transition, None)?;
        Ok(transition)
    }

    /// Fetch a watched Dexie offer and persist the resolved lifecycle transition.
    ///
    /// # Errors
    ///
    /// Returns an error if Dexie lookup, transition resolve, or `SQLite` persist fails.
    pub async fn fetch_and_apply(
        &self,
        dexie: &DexieClient,
        market_id: &str,
        offer_id: &str,
        current_state: &str,
        env: WatchedOfferTransitionEnv<'_>,
    ) -> SignerResult<(CycleOfferTransition, Option<i64>)> {
        let (transition, status, dexie_error) = resolve_watched_offer_transition_from_dexie_fetch(
            self.store,
            dexie,
            offer_id,
            current_state,
            env,
        )
        .await?;
        if let Some(error_text) = dexie_error.as_deref() {
            let persist_options = ReconcilePersistOptions {
                action: self.options.action,
                venue: self.options.venue,
                dexie_error: Some(error_text),
            };
            persist_resolved_watched_transition(
                self.store,
                market_id,
                offer_id,
                &transition,
                status,
                &persist_options,
            )?;
        } else {
            self.persist_transition(market_id, offer_id, &transition, status)?;
        }
        Ok((transition, status))
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_signals(
        &self,
        market_id: &str,
        offer_id: &str,
        current_state: &str,
        status: Option<i64>,
        signals: CoinsetTxSignals,
        cancel_submitted: Option<&CancelSubmittedContext>,
        last_seen_status: Option<i64>,
        now: DateTime<Utc>,
    ) -> SignerResult<CycleOfferTransition> {
        let chain_confirmed = chain_confirmed_tx_ids_for_transition(
            self.store,
            cancel_submitted,
            &signals.confirmed_tx_ids,
        )?;
        let transition = resolve_watched_offer_transition_from_signals(
            current_state,
            status,
            signals,
            &chain_confirmed,
            cancel_submitted,
            now,
        )?;
        self.persist_transition(market_id, offer_id, &transition, last_seen_status)?;
        Ok(transition)
    }
}

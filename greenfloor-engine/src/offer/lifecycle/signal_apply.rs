//! Shared resolve+persist for watched-offer Coinset, Dexie, and cancel-submitted signals.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::adapters::DexieClient;
use crate::cycle::reconcile::{
    resolve_watched_offer_transition_from_signals, CancelSubmittedContext, CoinsetTxSignals,
};
use crate::cycle::{
    resolve_missing_watched_offer_transition, unchanged_offer_transition, CycleOfferTransition,
};
use crate::error::SignerResult;
use crate::storage::{OfferStateListRow, SqliteStore};

use super::cancel_context::{
    cancel_submitted_context_for_offer, chain_confirmed_tx_ids_for_transition,
    preload_cancel_submitted_contexts,
};
use super::persist::{persist_offer_lifecycle_transition, ReconcilePersistOptions};
use super::reconcile_prep::{fetch_dexie_offer, DexieOfferFetch};
use super::transition::{transition_from_offer_body, WatchedOfferTransitionEnv};

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
    /// `dexie_error` is recorded on the persist audit when present.
    ///
    /// # Errors
    ///
    /// Returns an error if transition resolve or `SQLite` persist fails.
    pub fn apply_missing(
        &self,
        market_id: &str,
        offer_id: &str,
        current_state: &str,
        dexie_error: Option<&str>,
    ) -> SignerResult<CycleOfferTransition> {
        let transition = resolve_missing_watched_offer_transition(current_state)?;
        let overlaid = ReconcilePersistOptions {
            action: self.options.action,
            venue: self.options.venue,
            dexie_error: dexie_error.or(self.options.dexie_error),
        };
        persist_resolved_watched_transition(
            self.store,
            market_id,
            offer_id,
            &transition,
            None,
            &overlaid,
        )?;
        Ok(transition)
    }

    /// Fetch a watched Dexie offer and persist the resolved lifecycle transition.
    ///
    /// Transport failures and id-mismatched payloads persist an unchanged transition.
    ///
    /// # Errors
    ///
    /// Returns an error if transition resolve or `SQLite` persist fails.
    pub async fn fetch_and_apply(
        &self,
        dexie: &DexieClient,
        market_id: &str,
        offer_id: &str,
        current_state: &str,
        env: WatchedOfferTransitionEnv<'_>,
    ) -> SignerResult<(CycleOfferTransition, Option<i64>)> {
        match fetch_dexie_offer(dexie, offer_id).await {
            Ok(DexieOfferFetch::Found(offer_body)) => {
                self.apply_dexie_payload(market_id, offer_id, current_state, &offer_body, env)
            }
            Ok(DexieOfferFetch::Missing(error_text)) => {
                let transition =
                    self.apply_missing(market_id, offer_id, current_state, Some(&error_text))?;
                Ok((transition, None))
            }
            Ok(DexieOfferFetch::Mismatch) => self.persist_lookup_unchanged(
                market_id,
                offer_id,
                current_state,
                "dexie_lookup_error:dexie get_offer payload did not match local offer id",
            ),
            Err(err) => self.persist_lookup_unchanged(
                market_id,
                offer_id,
                current_state,
                format!("dexie_lookup_error:{err}"),
            ),
        }
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

    fn persist_lookup_unchanged(
        &self,
        market_id: &str,
        offer_id: &str,
        current_state: &str,
        reason: impl Into<String>,
    ) -> SignerResult<(CycleOfferTransition, Option<i64>)> {
        let transition = unchanged_offer_transition(current_state, reason)?;
        self.persist_transition(market_id, offer_id, &transition, None)?;
        Ok((transition, None))
    }
}

#[cfg(test)]
mod tests {
    use super::{WatchedOfferReconciler, WatchedOfferTransitionEnv};
    use crate::adapters::DexieClient;
    use crate::offer::lifecycle::ReconcilePersistOptions;
    use crate::storage::SqliteStore;
    use tempfile::tempdir;

    #[tokio::test]
    async fn fetch_and_apply_expires_on_dexie_404() {
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
        let options = ReconcilePersistOptions {
            action: "test",
            venue: Some(crate::config::Venue::Dexie),
            dexie_error: None,
        };
        let reconciler = WatchedOfferReconciler::new(&store, &options);
        let (transition, status) = reconciler
            .fetch_and_apply(
                &dexie,
                "m1",
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
    }

    #[tokio::test]
    async fn fetch_and_apply_mismatch_leaves_state_unchanged() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/offers/local-id")
            .with_status(200)
            .with_body(r#"{"offer":{"id":"other-id","status":1}}"#)
            .create();
        let dexie = DexieClient::new(server.url());
        let options = ReconcilePersistOptions {
            action: "test",
            venue: Some(crate::config::Venue::Dexie),
            dexie_error: None,
        };
        let reconciler = WatchedOfferReconciler::new(&store, &options);
        let (transition, status) = reconciler
            .fetch_and_apply(
                &dexie,
                "m1",
                "local-id",
                "open",
                WatchedOfferTransitionEnv::at_now(None),
            )
            .await
            .expect("transition");
        assert!(!transition.changed);
        assert_eq!(
            transition.new_state,
            crate::cycle::ReconcileState::parse("open").expect("state")
        );
        assert!(status.is_none());
    }

    #[tokio::test]
    async fn fetch_and_apply_transport_error_leaves_state_unchanged() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/offers/bad-json")
            .with_status(200)
            .with_body("not-json")
            .create();
        let dexie = DexieClient::new(server.url());
        let options = ReconcilePersistOptions {
            action: "test",
            venue: Some(crate::config::Venue::Dexie),
            dexie_error: None,
        };
        let reconciler = WatchedOfferReconciler::new(&store, &options);
        let (transition, status) = reconciler
            .fetch_and_apply(
                &dexie,
                "m1",
                "bad-json",
                "open",
                WatchedOfferTransitionEnv::at_now(None),
            )
            .await
            .expect("transition");
        assert!(!transition.changed);
        assert!(transition.reason.contains("dexie_lookup_error"));
        assert!(status.is_none());
    }
}

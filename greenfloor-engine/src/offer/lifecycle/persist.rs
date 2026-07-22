use serde_json::{json, Value};
use tracing::Level;

use crate::config::Venue;
use crate::cycle::lifecycle::OfferSignal;
use crate::cycle::CycleOfferTransition;
use crate::error::SignerResult;
use crate::operator_log::{LogContext, OFFER_LIFECYCLE_TRANSITION, TAKER_DETECTION};
use crate::storage::SqliteStore;

pub struct ReconcilePersistOptions<'a> {
    pub action: &'a str,
    pub venue: Option<Venue>,
    pub dexie_error: Option<&'a str>,
}

/// Persist offer lifecycle transition.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn persist_offer_lifecycle_transition(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    transition: &CycleOfferTransition,
    last_seen_status: Option<i64>,
    options: &ReconcilePersistOptions<'_>,
) -> SignerResult<()> {
    if transition.new_state.is_terminal() {
        store.immediate_transaction("offer_lifecycle_terminal", |store| {
            store.upsert_offer_state(
                offer_id,
                market_id,
                &transition.new_state.as_str(),
                last_seen_status,
            )?;
            store.clear_offer_coin_watches(offer_id)?;
            Ok(())
        })?;
    } else {
        store.upsert_offer_state(
            offer_id,
            market_id,
            &transition.new_state.as_str(),
            last_seen_status,
        )?;
    }
    let mut payload = json!({
        "offer_id": offer_id,
        "market_id": market_id,
        "old_state": transition.old_state.as_str(),
        "new_state": transition.new_state.as_str(),
        "changed": transition.changed,
        "reason": transition.reason,
        "signal": transition.signal.map(OfferSignal::as_str),
        "signal_source": transition.signal_source,
        "last_seen_status": last_seen_status,
        "dexie_status": last_seen_status,
        "coinset_tx_ids": transition.coinset_tx_ids,
        "coinset_confirmed_tx_ids": transition.coinset_confirmed_tx_ids,
        "coinset_mempool_tx_ids": transition.coinset_mempool_tx_ids,
        "taker_signal": transition.taker_signal,
        "taker_diagnostic": transition.taker_diagnostic,
        "action": options.action,
    });
    if let Some(venue) = options.venue {
        if let Value::Object(obj) = &mut payload {
            obj.insert(
                "venue".to_string(),
                Value::String(venue.as_str().to_string()),
            );
        }
    }
    if let Some(error) = options.dexie_error {
        if let Value::Object(obj) = &mut payload {
            obj.insert("dexie_error".to_string(), Value::String(error.to_string()));
        }
    }
    LogContext::MARKET_CYCLE.audit(store, OFFER_LIFECYCLE_TRANSITION, &payload, Some(market_id))?;
    if transition.taker_signal != "none" {
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            Level::INFO,
            "taker detection",
            TAKER_DETECTION,
            &json!({
                "offer_id": offer_id,
                "market_id": market_id,
                "venue": options.venue.unwrap_or(Venue::Dexie).as_str(),
                "signal": transition.taker_signal,
                "advisory_diagnostic": transition.taker_diagnostic,
                "old_state": transition.old_state.as_str(),
                "new_state": transition.new_state.as_str(),
                "last_seen_status": last_seen_status,
                "signal_source": transition.signal_source,
                "coinset_confirmed_tx_ids": transition.coinset_confirmed_tx_ids,
            }),
            Some(market_id),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::lifecycle::{OfferLifecycleState, OfferSignal};
    use crate::cycle::reconcile::ReconcileState;
    use tempfile::tempdir;

    fn open_store() -> (tempfile::TempDir, SqliteStore) {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        (dir, store)
    }

    fn transition(
        old: ReconcileState,
        new: ReconcileState,
        signal: Option<OfferSignal>,
    ) -> CycleOfferTransition {
        CycleOfferTransition {
            old_state: old,
            new_state: new,
            reason: "test".to_string(),
            signal_source: "test".to_string(),
            signal,
            changed: true,
            immediate_requeue: false,
            taker_signal: "none".to_string(),
            taker_diagnostic: "none".to_string(),
            coinset_tx_ids: Vec::new(),
            coinset_confirmed_tx_ids: Vec::new(),
            coinset_mempool_tx_ids: Vec::new(),
        }
    }

    #[test]
    fn terminal_persist_clears_offer_coin_watches() {
        let (_dir, store) = open_store();
        let offer_id = "ab".repeat(32);
        let coin = "cd".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "mempool_observed", None)
            .expect("upsert");
        store
            .replace_offer_coin_watches(&offer_id, "m1", std::slice::from_ref(&coin), &[])
            .expect("watch");
        persist_offer_lifecycle_transition(
            &store,
            "m1",
            &offer_id,
            &transition(
                ReconcileState::Lifecycle(OfferLifecycleState::MempoolObserved),
                ReconcileState::Lifecycle(OfferLifecycleState::TxBlockConfirmed),
                Some(OfferSignal::TxConfirmed),
            ),
            Some(4),
            &ReconcilePersistOptions {
                action: "test",
                venue: None,
                dexie_error: None,
            },
        )
        .expect("persist");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "tx_block_confirmed");
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("watches")
            .is_empty());
    }

    #[test]
    fn non_terminal_persist_keeps_offer_coin_watches() {
        let (_dir, store) = open_store();
        let offer_id = "ab".repeat(32);
        let coin = "cd".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("upsert");
        store
            .replace_offer_coin_watches(&offer_id, "m1", std::slice::from_ref(&coin), &[])
            .expect("watch");
        persist_offer_lifecycle_transition(
            &store,
            "m1",
            &offer_id,
            &transition(
                ReconcileState::Lifecycle(OfferLifecycleState::Open),
                ReconcileState::Lifecycle(OfferLifecycleState::MempoolObserved),
                Some(OfferSignal::MempoolSeen),
            ),
            None,
            &ReconcilePersistOptions {
                action: "test",
                venue: None,
                dexie_error: None,
            },
        )
        .expect("persist");
        let watched = store
            .list_watched_coin_ids_for_market("m1")
            .expect("watches");
        assert!(watched.contains(&coin));
    }
}

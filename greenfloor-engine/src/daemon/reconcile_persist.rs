use serde_json::{json, Value};

use crate::cycle::CycleOfferTransition;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

pub(crate) struct ReconcilePersistOptions<'a> {
    pub action: &'a str,
    pub venue: Option<&'a str>,
    pub dexie_error: Option<&'a str>,
}

pub(crate) fn persist_offer_lifecycle_transition(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    transition: &CycleOfferTransition,
    last_seen_status: Option<i64>,
    options: ReconcilePersistOptions<'_>,
) -> SignerResult<()> {
    store.upsert_offer_state(offer_id, market_id, &transition.new_state, last_seen_status)?;
    let mut payload = json!({
        "offer_id": offer_id,
        "market_id": market_id,
        "old_state": transition.old_state,
        "new_state": transition.new_state,
        "changed": transition.changed,
        "reason": transition.reason,
        "signal": transition.signal,
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
            obj.insert("venue".to_string(), Value::String(venue.to_string()));
        }
    }
    if let Some(error) = options.dexie_error {
        if let Value::Object(obj) = &mut payload {
            obj.insert("dexie_error".to_string(), Value::String(error.to_string()));
        }
    }
    store.add_audit_event("offer_lifecycle_transition", &payload, Some(market_id))?;
    if transition.taker_signal != "none" {
        store.add_audit_event(
            "taker_detection",
            &json!({
                "offer_id": offer_id,
                "market_id": market_id,
                "venue": options.venue.unwrap_or("dexie"),
                "signal": transition.taker_signal,
                "advisory_diagnostic": transition.taker_diagnostic,
                "old_state": transition.old_state,
                "new_state": transition.new_state,
                "last_seen_status": last_seen_status,
                "signal_source": transition.signal_source,
                "coinset_confirmed_tx_ids": transition.coinset_confirmed_tx_ids,
            }),
            Some(market_id),
        )?;
    }
    Ok(())
}

use serde_json::{json, Value};

use super::sqlite::{OfferPostPersistRecord, SqliteStore};
use crate::cycle::OfferLifecycleState;
use crate::error::SignerResult;

pub fn persist_offer_post_records(
    store: &SqliteStore,
    records: &[OfferPostPersistRecord],
) -> SignerResult<()> {
    for record in records {
        store.upsert_offer_state(
            &record.offer_id,
            &record.market_id,
            OfferLifecycleState::Open.as_str(),
            None,
        )?;
        let mut audit_event = json!({
            "market_id": record.market_id,
            "planned_count": 1,
            "executed_count": 1,
            "items": [{
                "size": record.size_base_units,
                "side": record.side,
                "status": "executed",
                "reason": format!("{}_post_success", record.publish_venue),
                "offer_id": record.offer_id,
                "attempts": 1,
            }],
            "venue": record.publish_venue,
            "resolved_base_asset_id": record.resolved_base_asset_id,
            "resolved_quote_asset_id": record.resolved_quote_asset_id,
        });
        if let Value::Object(extra) = &record.created_extra {
            if let Value::Object(audit_obj) = &mut audit_event {
                for (key, value) in extra {
                    audit_obj.insert(key.clone(), value.clone());
                }
            }
        }
        store.add_audit_event(
            "strategy_offer_execution",
            &audit_event,
            Some(record.market_id.as_str()),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn persist_offer_post_records_writes_offer_state_and_audit_event() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        let store = SqliteStore::open(&db_path).expect("open");

        persist_offer_post_records(
            &store,
            &[OfferPostPersistRecord {
                offer_id: "offer-123".to_string(),
                market_id: "m1".to_string(),
                side: "sell".to_string(),
                size_base_units: 10,
                publish_venue: "dexie".to_string(),
                resolved_base_asset_id: "a1".to_string(),
                resolved_quote_asset_id: "xch".to_string(),
                created_extra: json!({"execution_mode": "direct"}),
            }],
        )
        .expect("persist");

        let state = store
            .list_offer_state_details("m1", 10)
            .expect("states")
            .into_iter()
            .find(|row| row.offer_id == "offer-123")
            .expect("offer row");
        assert_eq!(state.state, "open");
    }
}

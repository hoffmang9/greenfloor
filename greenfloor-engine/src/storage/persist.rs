use super::sqlite::{OfferPostPersistRecord, SqliteStore};
use crate::cycle::OfferLifecycleState;
use crate::error::SignerResult;

/// Upsert offer lifecycle state for one posted offer record.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn upsert_offer_post_record(
    store: &SqliteStore,
    record: &OfferPostPersistRecord,
) -> SignerResult<()> {
    store.upsert_offer_state_with_metadata_at(
        &record.offer_id,
        &record.market_id,
        OfferLifecycleState::Open.as_str(),
        None,
        &super::sqlite::utcnow_iso(),
        super::sqlite::OfferCancelWrite {
            fields: Some(&record.cancel_fields),
            execution_mode: record.execution_mode,
            ..Default::default()
        },
    )?;
    store.replace_offer_coin_watches(
        &record.offer_id,
        &record.market_id,
        &record.watched_coin_ids,
        &record.watched_p2s,
    )?;
    Ok(())
}

/// Persist offer post records (`SQLite` offer state only; tracing lives in dispatch layer).
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn persist_offer_post_records(
    store: &SqliteStore,
    records: &[OfferPostPersistRecord],
) -> SignerResult<()> {
    for record in records {
        upsert_offer_post_record(store, record)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::types::{OfferExecutionMode, PresplitCancelFields};
    use serde_json::json;

    #[test]
    fn persist_offer_post_records_writes_offer_state() {
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
                created_extra: json!({}),
                cancel_fields: PresplitCancelFields::default(),
                execution_mode: Some(OfferExecutionMode::Direct),
                watched_coin_ids: vec!["ab".repeat(32)],
                watched_p2s: Vec::new(),
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
        let watched = store
            .list_watched_coin_ids_for_market("m1")
            .expect("watches");
        assert!(watched.contains(&"ab".repeat(32)));
    }

    #[test]
    fn persist_offer_post_records_writes_presplit_cancel_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        let store = SqliteStore::open(&db_path).expect("open");

        persist_offer_post_records(
            &store,
            &[OfferPostPersistRecord {
                offer_id: "offer-presplit".to_string(),
                market_id: "m1".to_string(),
                side: "sell".to_string(),
                size_base_units: 10,
                publish_venue: "dexie".to_string(),
                resolved_base_asset_id: "a1".to_string(),
                resolved_quote_asset_id: "xch".to_string(),
                created_extra: json!({}),
                cancel_fields: PresplitCancelFields {
                    input_coin_id: Some("c".repeat(64)),
                    fixed_delegated_puzzle_hash: Some("d".repeat(64)),
                },
                execution_mode: Some(OfferExecutionMode::PresplitExisting),
                watched_coin_ids: Vec::new(),
                watched_p2s: Vec::new(),
            }],
        )
        .expect("persist");

        let metadata = store
            .offer_cancel_metadata_for_id("offer-presplit")
            .expect("fields")
            .expect("row");
        assert_eq!(
            metadata.fields.input_coin_id.as_deref(),
            Some("c".repeat(64).as_str())
        );
        assert_eq!(
            metadata.fields.fixed_delegated_puzzle_hash.as_deref(),
            Some("d".repeat(64).as_str())
        );
        assert_eq!(
            metadata.execution_mode,
            Some(OfferExecutionMode::PresplitExisting)
        );
    }

    #[test]
    fn upsert_offer_post_record_empty_watches_clears_prior_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        let offer_id = "offer-clear";
        let coin = "ab".repeat(32);
        store
            .upsert_offer_state(offer_id, "m1", "open", None)
            .expect("seed state");
        store
            .replace_offer_coin_watches(offer_id, "m1", std::slice::from_ref(&coin), &[])
            .expect("seed watch");
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("list")
            .contains(&coin));

        upsert_offer_post_record(
            &store,
            &OfferPostPersistRecord {
                offer_id: offer_id.to_string(),
                market_id: "m1".to_string(),
                side: "sell".to_string(),
                size_base_units: 10,
                publish_venue: "coinset".to_string(),
                resolved_base_asset_id: "a1".to_string(),
                resolved_quote_asset_id: "xch".to_string(),
                created_extra: json!({}),
                cancel_fields: PresplitCancelFields::default(),
                execution_mode: Some(OfferExecutionMode::Direct),
                watched_coin_ids: Vec::new(),
                watched_p2s: Vec::new(),
            },
        )
        .expect("persist empty watches");

        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("cleared")
            .is_empty());
    }
}

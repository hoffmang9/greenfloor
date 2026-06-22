use super::sqlite::{OfferCancelMetadataRow, OfferPostPersistRecord, SqliteStore};
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
    let cancel_metadata = OfferCancelMetadataRow {
        presplit_input_coin_id: record.presplit_input_coin_id.clone(),
        fixed_delegated_puzzle_hash: record.fixed_delegated_puzzle_hash.clone(),
        execution_mode: record.execution_mode.clone(),
    };
    store.upsert_offer_state_with_metadata_at(
        &record.offer_id,
        &record.market_id,
        OfferLifecycleState::Open.as_str(),
        None,
        &super::sqlite::utcnow_iso(),
        Some(&cancel_metadata),
    )
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
                created_extra: json!({"execution_mode": "direct"}),
                presplit_input_coin_id: None,
                fixed_delegated_puzzle_hash: None,
                execution_mode: Some("direct".to_string()),
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

    #[test]
    fn persist_offer_post_records_writes_presplit_cancel_metadata() {
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
                created_extra: json!({"execution_mode": "presplit_existing"}),
                presplit_input_coin_id: Some("c".repeat(64)),
                fixed_delegated_puzzle_hash: Some("d".repeat(64)),
                execution_mode: Some("presplit_existing".to_string()),
            }],
        )
        .expect("persist");

        let metadata = store
            .offer_cancel_metadata_for_id("offer-presplit")
            .expect("metadata")
            .expect("row");
        assert_eq!(
            metadata.presplit_input_coin_id.as_deref(),
            Some("c".repeat(64).as_str())
        );
        assert_eq!(
            metadata.fixed_delegated_puzzle_hash.as_deref(),
            Some("d".repeat(64).as_str())
        );
        assert_eq!(
            metadata.execution_mode.as_deref(),
            Some("presplit_existing")
        );
    }
}

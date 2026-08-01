use tempfile::tempdir;

use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
use crate::storage::sqlite::{OfferCancelWrite, OfferListingWrite, SqliteStore};

use super::super::utcnow_iso;
use super::helpers::column_exists;

#[test]
fn migration_drops_legacy_presplit_input_coin_id() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let offer_id = "ab".repeat(32);
    let coin = "cd".repeat(32);
    {
        let raw_db = rusqlite::Connection::open(&db_path).expect("open raw");
        raw_db
            .execute_batch(
                r"
                CREATE TABLE offer_state (
                  offer_id TEXT PRIMARY KEY,
                  market_id TEXT NOT NULL,
                  state TEXT NOT NULL,
                  last_seen_status INTEGER NULL,
                  updated_at TEXT NOT NULL,
                  presplit_input_coin_id TEXT NULL
                );
                ",
            )
            .expect("legacy schema");
        raw_db
            .execute(
                r"
                INSERT INTO offer_state (
                  offer_id, market_id, state, last_seen_status, updated_at, presplit_input_coin_id
                ) VALUES (?1, 'm1', 'open', NULL, '2020-01-01T00:00:00Z', ?2)
                ",
                rusqlite::params![offer_id, coin],
            )
            .expect("seed legacy row");
    }
    let store = SqliteStore::open(&db_path).expect("open migrates");
    assert!(
        !column_exists(&store.conn, "offer_state", "presplit_input_coin_id").expect("pragma"),
        "legacy column must be dropped"
    );
    assert!(
        column_exists(&store.conn, "offer_state", "cancel_input_coin_id").expect("pragma"),
        "canonical column must remain"
    );
    let meta = store
        .offer_cancel_metadata_for_id(&offer_id)
        .expect("metadata")
        .expect("row");
    assert_eq!(meta.fields.input_coin_id.as_deref(), Some(coin.as_str()));
}

#[test]
fn migration_heals_partial_coin_watch_with_missing_p2() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let offer_id = "ab".repeat(32);
    let coin = "cd".repeat(32);
    let meta_p2 = "ef".repeat(32);
    {
        let store = SqliteStore::open(&db_path).expect("open");
        let fields = OfferCancelFields {
            input_coin_id: Some(coin.clone()),
            fixed_delegated_puzzle_hash: Some("aa".repeat(32)),
            maker_puzzle_hash: Some(meta_p2.clone()),
        };
        store
            .upsert_offer_state_with_metadata_at(
                &offer_id,
                "m1",
                "open",
                None,
                &utcnow_iso(),
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::PresplitExisting),
                    listing: OfferListingWrite::venue(Some("coinset")),
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert metadata");
        // Coin-only watch (pre-upgrade partial row).
        store
            .replace_offer_coin_watches(&offer_id, "m1", std::slice::from_ref(&coin), &[])
            .expect("coin watch");
        assert!(store
            .list_offer_ids_for_watched_coin(&meta_p2)
            .expect("no p2 yet")
            .is_empty());
        // Clear one-shot flag so reopen re-runs watch/venue backfill.
        store
            .conn
            .execute(
                "DELETE FROM schema_meta WHERE key IN ('watch_venue_backfill_v1', 'watch_venue_backfill_v2')",
                [],
            )
            .expect("clear schema_meta");
    }
    // Re-open re-runs one-shot backfill; INSERT OR IGNORE heals missing p2.
    let store = SqliteStore::open(&db_path).expect("reopen");
    assert_eq!(
        store
            .list_offer_ids_for_watched_coin(&meta_p2)
            .expect("healed p2"),
        vec![offer_id.clone()]
    );
    assert_eq!(
        store.list_offer_ids_for_watched_coin(&coin).expect("coin"),
        vec![offer_id]
    );
}

#[test]
fn migration_seeds_watches_from_cancel_metadata_when_absent() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let offer_id = "ab".repeat(32);
    let coin = "cd".repeat(32);
    let meta_p2 = "ef".repeat(32);
    {
        let store = SqliteStore::open(&db_path).expect("open");
        let fields = OfferCancelFields {
            input_coin_id: Some(coin.clone()),
            fixed_delegated_puzzle_hash: Some("aa".repeat(32)),
            maker_puzzle_hash: Some(meta_p2.clone()),
        };
        store
            .upsert_offer_state_with_metadata_at(
                &offer_id,
                "m1",
                "open",
                None,
                &utcnow_iso(),
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::PresplitExisting),
                    listing: OfferListingWrite::venue(Some("coinset")),
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert metadata");
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("empty")
            .is_empty());
        store
            .conn
            .execute(
                "DELETE FROM schema_meta WHERE key IN ('watch_venue_backfill_v1', 'watch_venue_backfill_v2')",
                [],
            )
            .expect("clear schema_meta");
    }
    let store = SqliteStore::open(&db_path).expect("reopen");
    let watched = store.list_watched_coin_ids_for_market("m1").expect("coins");
    assert!(watched.contains(&coin));
    assert!(!watched.contains(&meta_p2));
    assert_eq!(
        store
            .list_offer_ids_for_watched_coin(&meta_p2)
            .expect("meta p2"),
        vec![offer_id]
    );
}

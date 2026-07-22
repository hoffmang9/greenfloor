use super::*;

use serde_json::json;
use std::collections::HashMap;
use tempfile::tempdir;

fn open_test_store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test.sqlite");
    let store = SqliteStore::open(&db_path).expect("open");
    (dir, store)
}

fn insert_strategy_execution(
    store: &SqliteStore,
    market_id: &str,
    items: &serde_json::Value,
    created_at: &str,
) {
    store
        .add_audit_event_at(
            "strategy_offer_execution",
            &json!({ "items": items }),
            Some(market_id),
            created_at,
        )
        .expect("audit event");
}

#[test]
fn watchlist_includes_open_refresh_due_and_mempool_observed() {
    let (_dir, store) = open_test_store();
    store
        .upsert_offer_state("o1", "m1", "open", Some(0))
        .expect("upsert");
    store
        .upsert_offer_state("o2", "m1", "refresh_due", Some(0))
        .expect("upsert");
    store
        .upsert_offer_state("o3", "m1", "mempool_observed", Some(0))
        .expect("upsert");
    store
        .upsert_offer_state("o5", "m1", "cancel_submitted", Some(0))
        .expect("upsert");
    store
        .upsert_offer_state("o4", "m1", "cancelled", Some(3))
        .expect("upsert");
    let ids = watchlist_offer_ids(&store, "m1").expect("watchlist");
    assert!(ids.contains("o1"));
    assert!(ids.contains("o2"));
    assert!(ids.contains("o3"));
    assert!(ids.contains("o5"));
    assert!(!ids.contains("o4"));
}

#[test]
fn parse_event_created_at_accepts_rfc3339_and_sqlite_format() {
    let clock = Utc::now();
    assert!(time::parse_event_created_at(&clock.to_rfc3339()).is_some());
    let sql = clock.format("%Y-%m-%d %H:%M:%S").to_string();
    assert!(time::parse_event_created_at(&sql).is_some());
}

#[test]
fn is_recent_mempool_observed_offer_state_respects_age_window() {
    let clock = Utc::now();
    let recent = (clock - chrono::Duration::seconds(30)).to_rfc3339();
    let stale =
        (clock - chrono::Duration::seconds(time::RESEED_MEMPOOL_MAX_AGE_SECONDS + 1)).to_rfc3339();
    assert!(is_recent_mempool_observed_offer_state(&recent, clock));
    assert!(!is_recent_mempool_observed_offer_state(&stale, clock));
}

#[test]
fn active_offer_counts_by_size_uses_offer_state_and_size_mapping() {
    let (_dir, store) = open_test_store();
    let clock = Utc::now();
    let clock_iso = clock.to_rfc3339();
    store
        .upsert_offer_state_at("one-1", "m1", "open", Some(0), &clock_iso)
        .expect("upsert");
    store
        .upsert_offer_state_at("ten-1", "m1", "refresh_due", Some(0), &clock_iso)
        .expect("upsert");
    store
        .upsert_offer_state_at("hundred-1", "m1", "mempool_observed", Some(0), &clock_iso)
        .expect("upsert");
    store
        .upsert_offer_state_at("unknown-1", "m1", "open", Some(0), &clock_iso)
        .expect("upsert");
    insert_strategy_execution(
        &store,
        "m1",
        &json!([
            {"offer_id": "one-1", "size": 1, "status": "executed"},
            {"offer_id": "ten-1", "size": 10, "status": "executed"},
            {"offer_id": "hundred-1", "size": 100, "status": "executed"},
        ]),
        &clock_iso,
    );

    let (counts, unmapped) =
        active_offer_counts_by_size_at(&store, "m1", None, &[], clock).expect("counts");

    assert_eq!(counts.get(&1), Some(&1));
    assert_eq!(counts.get(&10), Some(&1));
    assert_eq!(counts.get(&100), Some(&1));
    assert_eq!(unmapped, 1);
}

#[test]
fn active_offer_counts_by_size_counts_cli_posted_offer() {
    let (_dir, store) = open_test_store();
    let clock = Utc::now();
    let clock_iso = clock.to_rfc3339();
    store
        .upsert_offer_state_at("cli-hundred-1", "m1", "open", Some(0), &clock_iso)
        .expect("upsert");
    store
        .add_audit_event_at(
            "strategy_offer_execution",
            &json!({
                "market_id": "m1",
                "planned_count": 1,
                "executed_count": 1,
                "items": [{
                    "size": 100,
                    "status": "executed",
                    "reason": "dexie_post_success",
                    "offer_id": "cli-hundred-1",
                    "attempts": 1,
                }],
            }),
            Some("m1"),
            &clock_iso,
        )
        .expect("audit");

    let (counts, unmapped) =
        active_offer_counts_by_size_at(&store, "m1", None, &[], clock).expect("counts");

    assert_eq!(counts.get(&100), Some(&1));
    assert_eq!(unmapped, 0);
}

#[test]
fn active_offer_counts_by_size_and_side_unknown_metadata_stays_unmapped() {
    let (_dir, store) = open_test_store();
    let clock = Utc::now();
    let clock_iso = clock.to_rfc3339();
    store
        .upsert_offer_state_at("offer-unknown-side", "m1", "open", Some(0), &clock_iso)
        .expect("upsert");

    let (buy_counts, sell_counts, unmapped) =
        active_offer_counts_by_size_and_side_at(&store, "m1", None, &[], clock).expect("counts");

    assert_eq!(buy_counts.get(&1), Some(&0));
    assert_eq!(sell_counts.get(&1), Some(&0));
    assert_eq!(unmapped, 1);
}

#[test]
fn active_offer_counts_by_size_and_side_malformed_side_stays_unmapped() {
    let (_dir, store) = open_test_store();
    let clock = Utc::now();
    let clock_iso = clock.to_rfc3339();
    store
        .upsert_offer_state_at("offer-bad-side", "m1", "open", Some(0), &clock_iso)
        .expect("upsert");
    store
        .upsert_offer_state_at("offer-missing-side", "m1", "open", Some(0), &clock_iso)
        .expect("upsert");
    insert_strategy_execution(
        &store,
        "m1",
        &json!([
            {
                "offer_id": "offer-bad-side",
                "size": 10,
                "status": "executed",
                "side": "not-a-side",
            },
            {
                "offer_id": "offer-missing-side",
                "size": 10,
                "status": "executed",
            },
        ]),
        &clock_iso,
    );

    let (_buy_counts, _sell_counts, unmapped) =
        active_offer_counts_by_size_and_side_at(&store, "m1", None, &[], clock).expect("counts");

    assert_eq!(unmapped, 2);
}

#[test]
fn active_offer_counts_by_size_uses_dexie_hint_for_beyond_cap_offer() {
    let (_dir, store) = open_test_store();
    let clock = Utc::now();
    let clock_iso = clock.to_rfc3339();
    store
        .upsert_offer_state_at("beyond-cap-hundred", "m1", "open", Some(0), &clock_iso)
        .expect("upsert");

    let (counts_without, unmapped_without) = active_offer_counts_by_size_at(
        &store,
        "m1",
        Some(&HashMap::<String, i64>::default()),
        &[],
        clock,
    )
    .expect("counts");
    assert_eq!(counts_without.get(&100), Some(&0));
    assert_eq!(unmapped_without, 1);

    let dexie = HashMap::from([("beyond-cap-hundred".to_string(), 100)]);
    let (counts_with, unmapped_with) =
        active_offer_counts_by_size_at(&store, "m1", Some(&dexie), &[], clock).expect("counts");
    assert_eq!(counts_with.get(&100), Some(&1));
    assert_eq!(unmapped_with, 0);
}

#[test]
fn active_offer_counts_by_size_foreign_offer_stays_unmapped() {
    let (_dir, store) = open_test_store();
    let clock = Utc::now();
    let clock_iso = clock.to_rfc3339();
    store
        .upsert_offer_state_at("ours-100", "m1", "open", Some(0), &clock_iso)
        .expect("upsert");
    store
        .upsert_offer_state_at("foreign-100", "m1", "open", Some(0), &clock_iso)
        .expect("upsert");
    insert_strategy_execution(
        &store,
        "m1",
        &json!([{"offer_id": "ours-100", "size": 100, "status": "executed"}]),
        &clock_iso,
    );

    let (counts, unmapped) =
        active_offer_counts_by_size_at(&store, "m1", None, &[], clock).expect("counts");

    assert_eq!(counts.get(&100), Some(&1));
    assert_eq!(unmapped, 1);
}

#[test]
fn active_offer_counts_by_size_tracks_non_legacy_size() {
    let (_dir, store) = open_test_store();
    let clock = Utc::now();
    let clock_iso = clock.to_rfc3339();
    store
        .upsert_offer_state_at("ours-50", "m1", "open", Some(0), &clock_iso)
        .expect("upsert");
    insert_strategy_execution(
        &store,
        "m1",
        &json!([{"offer_id": "ours-50", "size": 50, "status": "executed"}]),
        &clock_iso,
    );

    let (counts, unmapped) =
        active_offer_counts_by_size_at(&store, "m1", None, &[1, 10, 50], clock).expect("counts");

    assert_eq!(counts.get(&50), Some(&1));
    assert_eq!(unmapped, 0);
}

#[test]
fn active_offer_counts_excludes_stale_pending_visibility_offer() {
    let (_dir, store) = open_test_store();
    let clock = Utc::now();
    let stale_created_at = (clock - chrono::Duration::minutes(5)).to_rfc3339();
    store
        .upsert_offer_state_at("pending-50", "m1", "open", Some(0), &clock.to_rfc3339())
        .expect("upsert");
    insert_strategy_execution(
        &store,
        "m1",
        &json!([{
            "offer_id": "pending-50",
            "size": 50,
            "status": "pending_visibility",
            "reason": "managed_offer_post_success",
        }]),
        &stale_created_at,
    );

    let (counts, unmapped) = active_offer_counts_by_size_at(
        &store,
        "m1",
        Some(&HashMap::<String, i64>::default()),
        &[50],
        clock,
    )
    .expect("counts");

    assert_eq!(counts.get(&50), Some(&0));
    assert_eq!(unmapped, 1);
}

#[test]
fn active_offer_counts_keeps_pending_visibility_offer_when_seen_on_dexie() {
    let (_dir, store) = open_test_store();
    let clock = Utc::now();
    let stale_created_at = (clock - chrono::Duration::minutes(5)).to_rfc3339();
    store
        .upsert_offer_state_at("pending-50", "m1", "open", Some(0), &clock.to_rfc3339())
        .expect("upsert");
    insert_strategy_execution(
        &store,
        "m1",
        &json!([{
            "offer_id": "pending-50",
            "size": 50,
            "status": "pending_visibility",
            "reason": "managed_offer_post_success",
        }]),
        &stale_created_at,
    );

    let dexie = HashMap::from([("pending-50".to_string(), 50)]);
    let (counts, unmapped) =
        active_offer_counts_by_size_at(&store, "m1", Some(&dexie), &[50], clock).expect("counts");

    assert_eq!(counts.get(&50), Some(&1));
    assert_eq!(unmapped, 0);
}

#[test]
fn active_offer_counts_keeps_pending_when_no_dexie_snapshot() {
    let (_dir, store) = open_test_store();
    let clock = Utc::now();
    let very_old = (clock - chrono::Duration::hours(1)).to_rfc3339();
    store
        .upsert_offer_state_at("pending-old", "m1", "open", Some(0), &clock.to_rfc3339())
        .expect("upsert");
    insert_strategy_execution(
        &store,
        "m1",
        &json!([{
            "offer_id": "pending-old",
            "size": 50,
            "status": "pending_visibility",
            "reason": "managed_offer_post_success",
        }]),
        &very_old,
    );

    let (counts, unmapped) =
        active_offer_counts_by_size_at(&store, "m1", None, &[50], clock).expect("counts");

    assert_eq!(counts.get(&50), Some(&1));
    assert_eq!(unmapped, 0);
}

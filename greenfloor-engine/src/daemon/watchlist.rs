use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::cycle::OfferLifecycleState;
use crate::error::SignerResult;
use crate::offer::request::normalize_offer_side;
use crate::storage::SqliteStore;

const RESEED_MEMPOOL_MAX_AGE_SECONDS: i64 = 3 * 60;
const PENDING_VISIBILITY_RECHECK_MAX_AGE_SECONDS: i64 = 2 * 60;

#[derive(Debug, Clone)]
struct OfferExecutionMetadata {
    size: i64,
    side: Option<String>,
    status: String,
    created_at: String,
}

pub fn watchlist_offer_ids(store: &SqliteStore, market_id: &str) -> SignerResult<HashSet<String>> {
    let tracked_states: HashSet<&str> = [
        OfferLifecycleState::Open.as_str(),
        OfferLifecycleState::RefreshDue.as_str(),
        "unknown_orphaned",
    ]
    .into_iter()
    .collect();
    let mut offer_ids = HashSet::new();
    for row in store.list_offer_state_details(market_id, 500)? {
        let state = row.state.trim().to_ascii_lowercase();
        if tracked_states.contains(state.as_str())
            || state == OfferLifecycleState::MempoolObserved.as_str()
        {
            offer_ids.insert(row.offer_id);
        }
    }
    Ok(offer_ids)
}

fn parse_event_created_at(value: &str) -> Option<DateTime<Utc>> {
    let raw = value.trim();
    if raw.is_empty() {
        return None;
    }
    let normalized = raw.replace('Z', "+00:00");
    DateTime::parse_from_rfc3339(&normalized)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|naive| naive.and_utc())
        })
}

fn parse_offer_side_metadata(value: Option<&str>) -> Option<String> {
    let side = value?.trim().to_ascii_lowercase();
    if side == "buy" || side == "sell" {
        Some(side)
    } else {
        None
    }
}

fn is_recent_mempool_observed_offer_state(updated_at: &str, clock: DateTime<Utc>) -> bool {
    let Some(parsed) = parse_event_created_at(updated_at) else {
        return false;
    };
    let age_seconds = (clock - parsed).num_seconds();
    (0..=RESEED_MEMPOOL_MAX_AGE_SECONDS).contains(&age_seconds)
}

fn recent_offer_metadata_by_offer_id(
    store: &SqliteStore,
    market_id: &str,
) -> SignerResult<HashMap<String, OfferExecutionMetadata>> {
    let events = store.list_recent_audit_events(
        Some(&["strategy_offer_execution"]),
        Some(market_id),
        1500,
    )?;
    let mut metadata_by_offer_id = HashMap::new();
    for event in events {
        let Some(payload) = event.payload.as_object() else {
            continue;
        };
        let Some(items) = payload.get("items").and_then(|value| value.as_array()) else {
            continue;
        };
        for item in items {
            let Some(item_obj) = item.as_object() else {
                continue;
            };
            let status = item_obj
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if status != "executed" && status != "pending_visibility" {
                continue;
            }
            let offer_id = item_obj
                .get("offer_id")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if offer_id.is_empty() {
                continue;
            }
            let size = item_obj
                .get("size")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            if size <= 0 {
                continue;
            }
            let side =
                parse_offer_side_metadata(item_obj.get("side").and_then(|value| value.as_str()));
            if metadata_by_offer_id.contains_key(&offer_id) {
                continue;
            }
            metadata_by_offer_id.insert(
                offer_id,
                OfferExecutionMetadata {
                    size,
                    side,
                    status,
                    created_at: event.created_at.clone(),
                },
            );
        }
    }
    Ok(metadata_by_offer_id)
}

fn is_pending_visibility_metadata(metadata: &OfferExecutionMetadata) -> bool {
    metadata.status == "pending_visibility"
}

fn is_stale_pending_visibility_offer(
    offer_id: &str,
    metadata: &OfferExecutionMetadata,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    clock: DateTime<Utc>,
) -> bool {
    if !is_pending_visibility_metadata(metadata) {
        return false;
    }
    let Some(dexie_sizes) = dexie_size_by_offer_id else {
        return false;
    };
    if dexie_sizes.contains_key(offer_id) {
        return false;
    }
    let Some(created_at) = parse_event_created_at(&metadata.created_at) else {
        return true;
    };
    (clock - created_at).num_seconds() > PENDING_VISIBILITY_RECHECK_MAX_AGE_SECONDS
}

fn active_offer_state_summary(
    store: &SqliteStore,
    market_id: &str,
    clock: DateTime<Utc>,
    limit: usize,
) -> SignerResult<(
    Vec<String>,
    HashMap<String, i64>,
    HashMap<String, OfferExecutionMetadata>,
)> {
    let offer_states = store.list_offer_state_details(market_id, limit)?;
    let mut state_counts: HashMap<String, i64> = HashMap::new();
    for row in &offer_states {
        let state = row.state.trim().to_ascii_lowercase();
        if state.is_empty() {
            continue;
        }
        *state_counts.entry(state).or_insert(0) += 1;
    }

    let active_states: HashSet<&str> = [
        OfferLifecycleState::Open.as_str(),
        OfferLifecycleState::RefreshDue.as_str(),
    ]
    .into_iter()
    .collect();
    let mut active_offer_ids = Vec::new();
    for row in &offer_states {
        let state = row.state.trim().to_ascii_lowercase();
        let offer_id = row.offer_id.trim();
        if offer_id.is_empty() {
            continue;
        }
        if active_states.contains(state.as_str()) {
            active_offer_ids.push(offer_id.to_string());
            continue;
        }
        if state == OfferLifecycleState::MempoolObserved.as_str()
            && is_recent_mempool_observed_offer_state(&row.updated_at, clock)
        {
            active_offer_ids.push(offer_id.to_string());
        }
    }
    let metadata = recent_offer_metadata_by_offer_id(store, market_id)?;
    Ok((active_offer_ids, state_counts, metadata))
}

pub fn active_offer_counts_by_size(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
) -> SignerResult<(BTreeMap<i64, i64>, u64)> {
    let (counts, _, unmapped) = active_offer_counts_by_size_detail(
        store,
        market_id,
        dexie_size_by_offer_id,
        tracked_sizes,
        Utc::now(),
    )?;
    Ok((counts, unmapped))
}

pub fn active_offer_counts_by_size_detail(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
    clock: DateTime<Utc>,
) -> SignerResult<(BTreeMap<i64, i64>, HashMap<String, i64>, u64)> {
    let (counts, unmapped) = active_offer_counts_by_size_at(
        store,
        market_id,
        dexie_size_by_offer_id,
        tracked_sizes,
        clock,
    )?;
    let (_, state_counts, _) = active_offer_state_summary(store, market_id, clock, 500)?;
    Ok((counts, state_counts, unmapped))
}

fn active_offer_counts_by_size_at(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
    clock: DateTime<Utc>,
) -> SignerResult<(BTreeMap<i64, i64>, u64)> {
    let normalized_sizes: Vec<i64> = tracked_sizes
        .iter()
        .copied()
        .filter(|size| *size > 0)
        .collect();
    let sizes = if normalized_sizes.is_empty() {
        vec![1, 10, 100]
    } else {
        normalized_sizes
    };
    let mut active_counts: BTreeMap<i64, i64> = sizes.iter().map(|size| (*size, 0)).collect();
    let (active_offer_ids, _state_counts, metadata_by_offer_id) =
        active_offer_state_summary(store, market_id, clock, 500)?;
    let mut active_unmapped = 0_u64;
    for offer_id in active_offer_ids {
        let metadata = metadata_by_offer_id.get(&offer_id);
        if let Some(meta) = metadata {
            if is_stale_pending_visibility_offer(&offer_id, meta, dexie_size_by_offer_id, clock) {
                active_unmapped += 1;
                continue;
            }
        }
        let size = metadata
            .map(|meta| meta.size)
            .or_else(|| dexie_size_by_offer_id.and_then(|map| map.get(&offer_id).copied()));
        let Some(size) = size else {
            active_unmapped += 1;
            continue;
        };
        if let Some(count) = active_counts.get_mut(&size) {
            *count += 1;
        } else {
            active_unmapped += 1;
        }
    }
    Ok((active_counts, active_unmapped))
}

pub fn active_offer_counts_by_size_and_side(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
) -> SignerResult<(BTreeMap<i64, i64>, BTreeMap<i64, i64>, u64)> {
    let (buy, sell, _, unmapped) = active_offer_counts_by_size_and_side_detail(
        store,
        market_id,
        dexie_size_by_offer_id,
        tracked_sizes,
        Utc::now(),
    )?;
    Ok((buy, sell, unmapped))
}

pub fn active_offer_counts_by_size_and_side_detail(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
    clock: DateTime<Utc>,
) -> SignerResult<(
    BTreeMap<i64, i64>,
    BTreeMap<i64, i64>,
    HashMap<String, i64>,
    u64,
)> {
    let (buy, sell, unmapped) = active_offer_counts_by_size_and_side_at(
        store,
        market_id,
        dexie_size_by_offer_id,
        tracked_sizes,
        clock,
    )?;
    let (_, state_counts, _) = active_offer_state_summary(store, market_id, clock, 500)?;
    Ok((buy, sell, state_counts, unmapped))
}

fn active_offer_counts_by_size_and_side_at(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
    clock: DateTime<Utc>,
) -> SignerResult<(BTreeMap<i64, i64>, BTreeMap<i64, i64>, u64)> {
    let normalized_sizes: Vec<i64> = tracked_sizes
        .iter()
        .copied()
        .filter(|size| *size > 0)
        .collect();
    let sizes = if normalized_sizes.is_empty() {
        vec![1, 10, 100]
    } else {
        normalized_sizes
    };
    let mut buy_counts: BTreeMap<i64, i64> = sizes.iter().map(|size| (*size, 0)).collect();
    let mut sell_counts: BTreeMap<i64, i64> = sizes.iter().map(|size| (*size, 0)).collect();
    let (active_offer_ids, _state_counts, metadata_by_offer_id) =
        active_offer_state_summary(store, market_id, clock, 500)?;
    let mut active_unmapped = 0_u64;
    for offer_id in active_offer_ids {
        let Some(metadata) = metadata_by_offer_id.get(&offer_id) else {
            active_unmapped += 1;
            continue;
        };
        if is_stale_pending_visibility_offer(&offer_id, metadata, dexie_size_by_offer_id, clock) {
            active_unmapped += 1;
            continue;
        }
        let Some(side) = metadata.side.as_deref() else {
            active_unmapped += 1;
            continue;
        };
        let normalized_side = normalize_offer_side(side);
        let mut size = metadata.size;
        if size <= 0 {
            size = dexie_size_by_offer_id
                .and_then(|map| map.get(&offer_id).copied())
                .unwrap_or(0);
        }
        if size <= 0 {
            active_unmapped += 1;
            continue;
        }
        let target = if normalized_side == "buy" {
            buy_counts.get_mut(&size)
        } else {
            sell_counts.get_mut(&size)
        };
        if let Some(count) = target {
            *count += 1;
        } else {
            active_unmapped += 1;
        }
    }
    Ok((buy_counts, sell_counts, active_unmapped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
        items: serde_json::Value,
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
            .upsert_offer_state("o4", "m1", "cancelled", Some(3))
            .expect("upsert");
        let ids = watchlist_offer_ids(&store, "m1").expect("watchlist");
        assert!(ids.contains("o1"));
        assert!(ids.contains("o2"));
        assert!(ids.contains("o3"));
        assert!(!ids.contains("o4"));
    }

    #[test]
    fn parse_event_created_at_accepts_rfc3339_and_sqlite_format() {
        let clock = Utc::now();
        assert!(parse_event_created_at(&clock.to_rfc3339()).is_some());
        let sql = clock.format("%Y-%m-%d %H:%M:%S").to_string();
        assert!(parse_event_created_at(&sql).is_some());
    }

    #[test]
    fn is_recent_mempool_observed_offer_state_respects_age_window() {
        let clock = Utc::now();
        let recent = (clock - chrono::Duration::seconds(30)).to_rfc3339();
        let stale =
            (clock - chrono::Duration::seconds(RESEED_MEMPOOL_MAX_AGE_SECONDS + 1)).to_rfc3339();
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
            json!([
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
            active_offer_counts_by_size_and_side_at(&store, "m1", None, &[], clock)
                .expect("counts");

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
            json!([
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
            active_offer_counts_by_size_and_side_at(&store, "m1", None, &[], clock)
                .expect("counts");

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

        let (counts_without, unmapped_without) =
            active_offer_counts_by_size_at(&store, "m1", Some(&HashMap::new()), &[], clock)
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
            json!([{"offer_id": "ours-100", "size": 100, "status": "executed"}]),
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
            json!([{"offer_id": "ours-50", "size": 50, "status": "executed"}]),
            &clock_iso,
        );

        let (counts, unmapped) =
            active_offer_counts_by_size_at(&store, "m1", None, &[1, 10, 50], clock)
                .expect("counts");

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
            json!([{
                "offer_id": "pending-50",
                "size": 50,
                "status": "pending_visibility",
                "reason": "managed_offer_post_success",
            }]),
            &stale_created_at,
        );

        let (counts, unmapped) =
            active_offer_counts_by_size_at(&store, "m1", Some(&HashMap::new()), &[50], clock)
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
            json!([{
                "offer_id": "pending-50",
                "size": 50,
                "status": "pending_visibility",
                "reason": "managed_offer_post_success",
            }]),
            &stale_created_at,
        );

        let dexie = HashMap::from([("pending-50".to_string(), 50)]);
        let (counts, unmapped) =
            active_offer_counts_by_size_at(&store, "m1", Some(&dexie), &[50], clock)
                .expect("counts");

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
            json!([{
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
}

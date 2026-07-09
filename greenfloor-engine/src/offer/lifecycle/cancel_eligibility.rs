//! Shared cancel eligibility policy for daemon and CLI paths.

use std::collections::{HashMap, HashSet};

use crate::cycle::ReconcileState;
use crate::error::SignerResult;
use crate::offer::dexie_payload::DEXIE_STATUS_OPEN;
use crate::storage::{OfferStateListRow, SqliteStore};

/// Dexie list status `0` (open on venue).
#[must_use]
pub fn dexie_status_open_for_cancel(status: i64) -> bool {
    status == DEXIE_STATUS_OPEN
}

/// Whether a persisted offer row is eligible for cancel selection.
#[must_use]
pub fn row_cancel_eligible(row: &OfferStateListRow) -> bool {
    ReconcileState::parse(&row.state).is_ok_and(|state| state.is_cancel_eligible())
}

/// Filter cancel-eligible rows into target offer ids.
///
/// - Non-Dexie offers (Coinset/splash): local cancel-eligible state is enough.
/// - Dexie-authoritative offers: require Dexie list status open via
///   `dexie_status_by_lookup_key`. Absent from the map → skip (pass an empty map
///   to exclude Dexie venue offers; use explicit `--offer-id` as operator override).
#[must_use]
pub fn filter_cancel_target_offer_ids(
    rows: &[OfferStateListRow],
    dexie_status_by_lookup_key: &HashMap<String, i64>,
) -> Vec<String> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        if !row_cancel_eligible(row) {
            continue;
        }
        let offer_id = row.offer_id.trim();
        if offer_id.is_empty() || !seen.insert(offer_id.to_string()) {
            continue;
        }
        if SqliteStore::is_dexie_authoritative_for_offer(offer_id, row.publish_venue.as_deref()) {
            match dexie_status_by_lookup_key.get(offer_id) {
                Some(status) if dexie_status_open_for_cancel(*status) => {
                    targets.push(offer_id.to_string());
                }
                _ => {}
            }
        } else {
            targets.push(offer_id.to_string());
        }
    }
    targets.sort();
    targets
}

/// Collect daemon cancel targets for one market.
///
/// # Errors
///
/// Returns an error if `SQLite` reads fail.
pub fn collect_market_cancel_target_offer_ids(
    store: &SqliteStore,
    market_id: &str,
    dexie_status_by_lookup_key: &HashMap<String, i64>,
) -> SignerResult<Vec<String>> {
    let clean_market = market_id.trim();
    if clean_market.is_empty() {
        return Ok(Vec::new());
    }
    let rows = store.list_offer_states(Some(clean_market), 5000)?;
    Ok(filter_cancel_target_offer_ids(
        &rows,
        dexie_status_by_lookup_key,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::OfferStateListRow;
    use tempfile::tempdir;

    fn row(offer_id: &str, state: &str) -> OfferStateListRow {
        OfferStateListRow {
            offer_id: offer_id.to_string(),
            market_id: "m1".to_string(),
            state: state.to_string(),
            last_seen_status: None,
            updated_at: String::new(),
            cancel_submitted_tx_id: None,
            cancel_submitted_at: None,
            publish_venue: None,
        }
    }

    #[test]
    fn row_cancel_eligible_allows_open_and_pending_visibility() {
        assert!(row_cancel_eligible(&row("a", "open")));
        assert!(row_cancel_eligible(&row("b", "pending_visibility")));
        assert!(!row_cancel_eligible(&row("c", "cancel_submitted")));
        assert!(!row_cancel_eligible(&row("d", "expired")));
    }

    #[test]
    fn collect_market_cancel_targets_includes_coinset_without_dexie_list() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_id = "ab".repeat(32);
        store
            .upsert_offer_state_with_metadata_at(
                &offer_id,
                "m1",
                "open",
                None,
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("coinset"),
                    ..Default::default()
                },
            )
            .expect("seed");
        let targets =
            collect_market_cancel_target_offer_ids(&store, "m1", &HashMap::new()).expect("targets");
        assert_eq!(targets, vec![offer_id]);
    }

    #[test]
    fn collect_market_cancel_targets_requires_dexie_open_status_for_dexie_venue() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        store
            .upsert_offer_state_with_metadata_at(
                "offer-dexie",
                "m1",
                "open",
                None,
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("dexie"),
                    ..Default::default()
                },
            )
            .expect("seed");
        let empty =
            collect_market_cancel_target_offer_ids(&store, "m1", &HashMap::new()).expect("empty");
        assert!(empty.is_empty());
        let mut status = HashMap::new();
        status.insert("offer-dexie".to_string(), 0);
        let targets =
            collect_market_cancel_target_offer_ids(&store, "m1", &status).expect("targets");
        assert_eq!(targets, vec!["offer-dexie".to_string()]);
    }

    #[test]
    fn collect_market_cancel_targets_matches_dexie_trade_id_key() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let trade_id = "cd".repeat(32);
        store
            .upsert_offer_state_with_metadata_at(
                &trade_id,
                "m1",
                "open",
                None,
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("dexie"),
                    ..Default::default()
                },
            )
            .expect("seed");
        let mut status = HashMap::new();
        status.insert(trade_id.clone(), 0);
        status.insert("bech32-list-id".to_string(), 0);
        let targets =
            collect_market_cancel_target_offer_ids(&store, "m1", &status).expect("targets");
        assert_eq!(targets, vec![trade_id]);
    }

    #[test]
    fn filter_cancel_targets_skips_null_venue_without_dexie_status() {
        // After venue backfill NULL is non-Dexie; without venue, authority is false.
        let rows = vec![row("legacy-open", "open")];
        let targets = filter_cancel_target_offer_ids(&rows, &HashMap::new());
        assert_eq!(targets, vec!["legacy-open".to_string()]);
    }
}

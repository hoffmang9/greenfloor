//! Shared cancel eligibility policy for daemon and CLI paths.

use std::collections::HashSet;

use crate::cycle::ReconcileState;
use crate::offer::dexie_payload::DEXIE_STATUS_OPEN;
use crate::storage::OfferStateListRow;

/// Dexie list status `0` (open on venue).
#[must_use]
pub fn dexie_status_open_for_cancel(status: i64) -> bool {
    status == DEXIE_STATUS_OPEN
}

/// Collect Dexie-open offer ids eligible for daemon cancel policy.
#[must_use]
pub fn collect_dexie_open_offer_ids(offers: &[(String, i64)]) -> Vec<String> {
    offers
        .iter()
        .filter_map(|(offer_id, status)| {
            let normalized_id = offer_id.trim();
            if normalized_id.is_empty() || !dexie_status_open_for_cancel(*status) {
                return None;
            }
            Some(normalized_id.to_string())
        })
        .collect()
}

/// Whether a persisted offer row is eligible for CLI `--cancel-open` selection.
#[must_use]
pub fn row_cancel_eligible(row: &OfferStateListRow) -> bool {
    ReconcileState::parse(&row.state).is_ok_and(|state| state.is_cancel_eligible())
}

/// Drop Dexie-open ids already in `cancel_submitted` (legacy filter without tx tracking).
#[must_use]
pub fn filter_out_cancel_submitted_state_ids(
    offer_ids: &[String],
    db_rows: &[OfferStateListRow],
) -> Vec<String> {
    let cancel_pending: HashSet<&str> = db_rows
        .iter()
        .filter_map(|row| {
            ReconcileState::parse(&row.state)
                .ok()
                .filter(ReconcileState::is_cancel_submitted)
                .map(|_| row.offer_id.as_str())
        })
        .collect();
    offer_ids
        .iter()
        .filter(|offer_id| !cancel_pending.contains(offer_id.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::OfferStateListRow;

    fn row(offer_id: &str, state: &str) -> OfferStateListRow {
        OfferStateListRow {
            offer_id: offer_id.to_string(),
            market_id: "m1".to_string(),
            state: state.to_string(),
            last_seen_status: None,
            updated_at: String::new(),
            cancel_submitted_tx_id: None,
        }
    }

    #[test]
    fn collect_dexie_open_offer_ids_skips_non_open() {
        let offers = vec![
            ("o1".to_string(), 0),
            ("o2".to_string(), 4),
            ("  ".to_string(), 0),
        ];
        assert_eq!(
            collect_dexie_open_offer_ids(&offers),
            vec!["o1".to_string()]
        );
    }

    #[test]
    fn filter_out_cancel_submitted_state_ids_skips_cancel_submitted() {
        let open_ids = vec!["o1".to_string(), "o2".to_string()];
        let db_rows = vec![row("o1", "open"), row("o2", "cancel_submitted")];
        assert_eq!(
            filter_out_cancel_submitted_state_ids(&open_ids, &db_rows),
            vec!["o1".to_string()]
        );
    }

    #[test]
    fn row_cancel_eligible_accepts_open_and_pending_visibility() {
        assert!(row_cancel_eligible(&row("o1", "open")));
        assert!(row_cancel_eligible(&row("o2", "pending_visibility")));
        assert!(!row_cancel_eligible(&row("o3", "cancel_submitted")));
    }
}

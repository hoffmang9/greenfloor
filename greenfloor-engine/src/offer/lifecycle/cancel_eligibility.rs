//! Shared cancel eligibility policy for daemon and CLI paths.

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
            cancel_submitted_at: None,
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
    fn row_cancel_eligible_accepts_open_and_pending_visibility() {
        assert!(row_cancel_eligible(&row("o1", "open")));
        assert!(row_cancel_eligible(&row("o2", "pending_visibility")));
        assert!(!row_cancel_eligible(&row("o3", "cancel_submitted")));
    }
}

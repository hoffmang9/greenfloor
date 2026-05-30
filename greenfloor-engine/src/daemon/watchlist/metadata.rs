use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::time::{parse_event_created_at, PENDING_VISIBILITY_RECHECK_MAX_AGE_SECONDS};

#[derive(Debug, Clone)]
pub(crate) struct OfferExecutionMetadata {
    pub size: i64,
    pub side: Option<String>,
    pub status: String,
    pub created_at: String,
}

pub(crate) fn parse_offer_side_metadata(value: Option<&str>) -> Option<String> {
    let side = value?.trim().to_ascii_lowercase();
    if side == "buy" || side == "sell" {
        Some(side)
    } else {
        None
    }
}

pub(crate) fn recent_offer_metadata_by_offer_id(
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

pub(crate) fn is_stale_pending_visibility_offer(
    offer_id: &str,
    metadata: &OfferExecutionMetadata,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    clock: DateTime<Utc>,
) -> bool {
    if metadata.status != "pending_visibility" {
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

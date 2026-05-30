use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};

use crate::offer::request::normalize_offer_side;

use super::metadata::{is_stale_pending_visibility_offer, OfferExecutionMetadata};

pub(crate) fn normalize_tracked_sizes(tracked_sizes: &[i64]) -> Vec<i64> {
    let normalized: Vec<i64> = tracked_sizes
        .iter()
        .copied()
        .filter(|size| *size > 0)
        .collect();
    if normalized.is_empty() {
        vec![1, 10, 100]
    } else {
        normalized
    }
}

pub(crate) struct SizeBucketCounts {
    pub counts: BTreeMap<i64, i64>,
    pub unmapped: u64,
}

pub(crate) struct SideBucketCounts {
    pub buy_counts: BTreeMap<i64, i64>,
    pub sell_counts: BTreeMap<i64, i64>,
    pub unmapped: u64,
}

pub(crate) fn bucket_active_offers_by_size(
    active_offer_ids: &[String],
    metadata_by_offer_id: &HashMap<String, OfferExecutionMetadata>,
    tracked_sizes: &[i64],
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    clock: DateTime<Utc>,
) -> SizeBucketCounts {
    let sizes = normalize_tracked_sizes(tracked_sizes);
    let mut counts: BTreeMap<i64, i64> = sizes.iter().map(|size| (*size, 0)).collect();
    let mut unmapped = 0_u64;
    for offer_id in active_offer_ids {
        let metadata = metadata_by_offer_id.get(offer_id);
        if let Some(meta) = metadata {
            if is_stale_pending_visibility_offer(offer_id, meta, dexie_size_by_offer_id, clock) {
                unmapped += 1;
                continue;
            }
        }
        let size = metadata
            .map(|meta| meta.size)
            .or_else(|| dexie_size_by_offer_id.and_then(|map| map.get(offer_id).copied()));
        let Some(size) = size else {
            unmapped += 1;
            continue;
        };
        if let Some(count) = counts.get_mut(&size) {
            *count += 1;
        } else {
            unmapped += 1;
        }
    }
    SizeBucketCounts { counts, unmapped }
}

pub(crate) fn bucket_active_offers_by_side(
    active_offer_ids: &[String],
    metadata_by_offer_id: &HashMap<String, OfferExecutionMetadata>,
    tracked_sizes: &[i64],
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    clock: DateTime<Utc>,
) -> SideBucketCounts {
    let sizes = normalize_tracked_sizes(tracked_sizes);
    let mut buy_counts: BTreeMap<i64, i64> = sizes.iter().map(|size| (*size, 0)).collect();
    let mut sell_counts: BTreeMap<i64, i64> = sizes.iter().map(|size| (*size, 0)).collect();
    let mut unmapped = 0_u64;
    for offer_id in active_offer_ids {
        let Some(metadata) = metadata_by_offer_id.get(offer_id) else {
            unmapped += 1;
            continue;
        };
        if is_stale_pending_visibility_offer(offer_id, metadata, dexie_size_by_offer_id, clock) {
            unmapped += 1;
            continue;
        }
        let Some(side) = metadata.side.as_deref() else {
            unmapped += 1;
            continue;
        };
        let normalized_side = normalize_offer_side(side);
        let mut size = metadata.size;
        if size <= 0 {
            size = dexie_size_by_offer_id
                .and_then(|map| map.get(offer_id).copied())
                .unwrap_or(0);
        }
        if size <= 0 {
            unmapped += 1;
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
            unmapped += 1;
        }
    }
    SideBucketCounts {
        buy_counts,
        sell_counts,
        unmapped,
    }
}

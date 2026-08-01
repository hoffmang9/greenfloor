use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};

use crate::offer::request::effective_offer_side;
use crate::storage::OfferListingFields;

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

fn listing_size(listing: Option<&OfferListingFields>) -> Option<i64> {
    listing.and_then(|fields| fields.size_base_units)
}

fn listing_side(listing: Option<&OfferListingFields>) -> Option<&str> {
    listing.and_then(|fields| fields.offer_side.as_deref())
}

/// Resolve size: `offer_state` listing columns first, then audit, then Dexie hint.
fn resolve_active_offer_size(
    offer_id: &str,
    listing: Option<&OfferListingFields>,
    metadata: Option<&OfferExecutionMetadata>,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
) -> Option<i64> {
    if let Some(size) = listing_size(listing) {
        return Some(size);
    }
    if let Some(meta) = metadata {
        if meta.size > 0 {
            return Some(meta.size);
        }
    }
    dexie_size_by_offer_id.and_then(|map| map.get(offer_id).copied())
}

pub(crate) fn bucket_active_offers_by_size(
    active_offer_ids: &[String],
    listing_by_offer_id: &HashMap<String, OfferListingFields>,
    metadata_by_offer_id: &HashMap<String, OfferExecutionMetadata>,
    tracked_sizes: &[i64],
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    clock: DateTime<Utc>,
) -> SizeBucketCounts {
    let sizes = normalize_tracked_sizes(tracked_sizes);
    let mut counts: BTreeMap<i64, i64> = sizes.iter().map(|size| (*size, 0)).collect();
    let mut unmapped = 0_u64;
    for offer_id in active_offer_ids {
        let listing = listing_by_offer_id.get(offer_id);
        let metadata = metadata_by_offer_id.get(offer_id);
        // Listing columns are authoritative; skip stale-pending only on audit-only rows.
        if listing_size(listing).is_none() {
            if let Some(meta) = metadata {
                if is_stale_pending_visibility_offer(offer_id, meta, dexie_size_by_offer_id, clock)
                {
                    unmapped += 1;
                    continue;
                }
            }
        }
        let Some(size) =
            resolve_active_offer_size(offer_id, listing, metadata, dexie_size_by_offer_id)
        else {
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
    listing_by_offer_id: &HashMap<String, OfferListingFields>,
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
        let listing = listing_by_offer_id.get(offer_id);
        let metadata = metadata_by_offer_id.get(offer_id);
        if listing_size(listing).is_none() && listing_side(listing).is_none() {
            if let Some(meta) = metadata {
                if is_stale_pending_visibility_offer(offer_id, meta, dexie_size_by_offer_id, clock)
                {
                    unmapped += 1;
                    continue;
                }
            }
        }
        let Some(offer_size) =
            resolve_active_offer_size(offer_id, listing, metadata, dexie_size_by_offer_id)
        else {
            unmapped += 1;
            continue;
        };
        let normalized_side = effective_offer_side(
            listing_side(listing).or_else(|| metadata.and_then(|meta| meta.side.as_deref())),
        );
        let target = if normalized_side == "buy" {
            buy_counts.get_mut(&offer_size)
        } else {
            sell_counts.get_mut(&offer_size)
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

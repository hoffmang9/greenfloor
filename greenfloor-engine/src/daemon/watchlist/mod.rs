mod counting;
mod metadata;
pub mod time;

use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::cycle::ReconcileState;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use counting::{bucket_active_offers_by_side, bucket_active_offers_by_size};
use metadata::{recent_offer_metadata_by_offer_id, OfferExecutionMetadata};
use time::is_recent_mempool_observed_offer_state;

type ActiveOfferStateSummary = (
    Vec<String>,
    HashMap<String, i64>,
    HashMap<String, crate::storage::OfferListingFields>,
    HashMap<String, OfferExecutionMetadata>,
);
type SizeCountDetail = (BTreeMap<i64, i64>, HashMap<String, i64>, u64);
type SideCountDetail = (
    BTreeMap<i64, i64>,
    BTreeMap<i64, i64>,
    HashMap<String, i64>,
    u64,
);
type SideCounts = (BTreeMap<i64, i64>, BTreeMap<i64, i64>, u64);

pub use time::RESEED_MEMPOOL_MAX_AGE_SECONDS;

/// Watchlist offer ids.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn watchlist_offer_ids(store: &SqliteStore, market_id: &str) -> SignerResult<HashSet<String>> {
    let mut offer_ids = HashSet::default();
    for row in store.list_offer_states(Some(market_id), 500)? {
        if ReconcileState::parse(&row.state).is_ok_and(|state| state.is_watched_for_reconcile()) {
            offer_ids.insert(row.offer_id);
        }
    }
    Ok(offer_ids)
}

/// Recent executed offer ids.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn recent_executed_offer_ids(
    store: &SqliteStore,
    market_id: &str,
) -> SignerResult<HashSet<String>> {
    let metadata = recent_offer_metadata_by_offer_id(store, market_id)?;
    Ok(metadata.into_keys().collect())
}

fn active_offer_state_summary(
    store: &SqliteStore,
    market_id: &str,
    clock: DateTime<Utc>,
) -> SignerResult<ActiveOfferStateSummary> {
    let offer_states = store.list_ladder_capacity_offer_states(market_id)?;
    let mut state_counts: HashMap<String, i64> = HashMap::default();
    let mut active_offer_ids = Vec::new();
    for row in &offer_states {
        let state = row.state.trim().to_ascii_lowercase();
        if state.is_empty() {
            continue;
        }
        *state_counts.entry(state.clone()).or_insert(0) += 1;
        let offer_id = row.offer_id.trim();
        if offer_id.is_empty() {
            continue;
        }
        let Ok(parsed) = ReconcileState::parse(&state) else {
            continue;
        };
        if parsed.counts_toward_ladder_capacity() {
            active_offer_ids.push(offer_id.to_string());
            continue;
        }
        if parsed.is_timed_ladder_capacity_candidate()
            && is_recent_mempool_observed_offer_state(&row.updated_at, clock)
        {
            active_offer_ids.push(offer_id.to_string());
        }
    }
    let listing = store.offer_listing_fields_by_offer_id(market_id)?;
    let metadata = recent_offer_metadata_by_offer_id(store, market_id)?;
    Ok((active_offer_ids, state_counts, listing, metadata))
}

/// Active offer counts by size.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn active_offer_counts_by_size(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
) -> SignerResult<(BTreeMap<i64, i64>, u64)> {
    let (counts, _state_counts, unmapped) = active_offer_counts_by_size_detail(
        store,
        market_id,
        dexie_size_by_offer_id,
        tracked_sizes,
        Utc::now(),
    )?;
    Ok((counts, unmapped))
}

/// Active offer counts by size detail.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn active_offer_counts_by_size_detail(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
    clock: DateTime<Utc>,
) -> SignerResult<SizeCountDetail> {
    active_offer_counts_by_size_at(
        store,
        market_id,
        dexie_size_by_offer_id,
        tracked_sizes,
        clock,
    )
}

fn active_offer_counts_by_size_at(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
    clock: DateTime<Utc>,
) -> SignerResult<SizeCountDetail> {
    let (active_offer_ids, state_counts, listing_by_offer_id, metadata_by_offer_id) =
        active_offer_state_summary(store, market_id, clock)?;
    let buckets = bucket_active_offers_by_size(
        &active_offer_ids,
        &listing_by_offer_id,
        &metadata_by_offer_id,
        tracked_sizes,
        dexie_size_by_offer_id,
        clock,
    );
    Ok((buckets.counts, state_counts, buckets.unmapped))
}

/// Active offer counts by size and side.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn active_offer_counts_by_size_and_side(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
) -> SignerResult<SideCounts> {
    let (buy, sell, _, unmapped) = active_offer_counts_by_size_and_side_detail(
        store,
        market_id,
        dexie_size_by_offer_id,
        tracked_sizes,
        Utc::now(),
    )?;
    Ok((buy, sell, unmapped))
}

/// Active offer counts by size and side detail.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn active_offer_counts_by_size_and_side_detail(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
    clock: DateTime<Utc>,
) -> SignerResult<SideCountDetail> {
    active_offer_counts_by_size_and_side_at(
        store,
        market_id,
        dexie_size_by_offer_id,
        tracked_sizes,
        clock,
    )
}

fn active_offer_counts_by_size_and_side_at(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
    clock: DateTime<Utc>,
) -> SignerResult<SideCountDetail> {
    let (active_offer_ids, state_counts, listing_by_offer_id, metadata_by_offer_id) =
        active_offer_state_summary(store, market_id, clock)?;
    let buckets = bucket_active_offers_by_side(
        &active_offer_ids,
        &listing_by_offer_id,
        &metadata_by_offer_id,
        tracked_sizes,
        dexie_size_by_offer_id,
        clock,
    );
    Ok((
        buckets.buy_counts,
        buckets.sell_counts,
        state_counts,
        buckets.unmapped,
    ))
}

#[cfg(test)]
mod tests;

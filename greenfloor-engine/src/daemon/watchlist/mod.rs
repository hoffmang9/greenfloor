mod counting;
mod metadata;
mod time;

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::cycle::OfferLifecycleState;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use counting::{bucket_active_offers_by_side, bucket_active_offers_by_size};
use metadata::{recent_offer_metadata_by_offer_id, OfferExecutionMetadata};
use time::is_recent_mempool_observed_offer_state;

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
    let (active_offer_ids, _state_counts, metadata_by_offer_id) =
        active_offer_state_summary(store, market_id, clock, 500)?;
    let buckets = bucket_active_offers_by_size(
        &active_offer_ids,
        &metadata_by_offer_id,
        tracked_sizes,
        dexie_size_by_offer_id,
        clock,
    );
    Ok((buckets.counts, buckets.unmapped))
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
    let (active_offer_ids, _state_counts, metadata_by_offer_id) =
        active_offer_state_summary(store, market_id, clock, 500)?;
    let buckets = bucket_active_offers_by_side(
        &active_offer_ids,
        &metadata_by_offer_id,
        tracked_sizes,
        dexie_size_by_offer_id,
        clock,
    );
    Ok((buckets.buy_counts, buckets.sell_counts, buckets.unmapped))
}

#[cfg(test)]
mod tests;

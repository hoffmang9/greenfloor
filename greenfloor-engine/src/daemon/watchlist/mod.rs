pub mod cache;

mod counting;
mod metadata;
pub mod time;

use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::cycle::OfferLifecycleState;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use crate::offer::dexie_payload::extract_coin_ids_from_offer_payload;
use counting::{bucket_active_offers_by_side, bucket_active_offers_by_size};
use metadata::{recent_offer_metadata_by_offer_id, OfferExecutionMetadata};
use time::is_recent_mempool_observed_offer_state;

type ActiveOfferStateSummary = (
    Vec<String>,
    HashMap<String, i64>,
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

pub fn watchlist_offer_ids(store: &SqliteStore, market_id: &str) -> SignerResult<HashSet<String>> {
    let tracked_states: HashSet<&str> = [
        OfferLifecycleState::Open.as_str(),
        OfferLifecycleState::RefreshDue.as_str(),
        "unknown_orphaned",
    ]
    .into_iter()
    .collect();
    let mut offer_ids = HashSet::default();
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

pub fn recent_executed_offer_ids(
    store: &SqliteStore,
    market_id: &str,
) -> SignerResult<HashSet<String>> {
    let metadata = recent_offer_metadata_by_offer_id(store, market_id)?;
    Ok(metadata.into_keys().collect())
}

pub fn watchlist_offer_ids_for_coin_tracking(
    store: &SqliteStore,
    market_id: &str,
) -> SignerResult<HashSet<String>> {
    let mut offer_ids = watchlist_offer_ids(store, market_id)?;
    offer_ids.extend(recent_executed_offer_ids(store, market_id)?);
    Ok(offer_ids)
}

fn active_offer_state_summary(
    store: &SqliteStore,
    market_id: &str,
    clock: DateTime<Utc>,
    limit: usize,
) -> SignerResult<ActiveOfferStateSummary> {
    let offer_states = store.list_offer_state_details(market_id, limit)?;
    let mut state_counts: HashMap<String, i64> = HashMap::default();
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
) -> SignerResult<SizeCountDetail> {
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

pub fn active_offer_counts_by_size_and_side_detail(
    store: &SqliteStore,
    market_id: &str,
    dexie_size_by_offer_id: Option<&HashMap<String, i64>>,
    tracked_sizes: &[i64],
    clock: DateTime<Utc>,
) -> SignerResult<SideCountDetail> {
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
) -> SignerResult<SideCounts> {
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

pub use cache::CoinWatchlistCache;

pub fn watched_coin_ids_for_market(cache: &CoinWatchlistCache, market_id: &str) -> HashSet<String> {
    cache.watched_coin_ids_for_market(market_id)
}

pub fn set_watched_coin_ids_for_market(
    cache: &CoinWatchlistCache,
    market_id: &str,
    coin_ids: HashSet<String>,
) {
    cache.set_watched_coin_ids_for_market(market_id, coin_ids);
}

pub fn match_watched_coin_ids(
    cache: &CoinWatchlistCache,
    observed_coin_ids: &[String],
) -> HashMap<String, Vec<String>> {
    cache.match_watched_coin_ids(observed_coin_ids)
}

pub fn update_market_coin_watchlist_from_offers(
    store: &SqliteStore,
    cache: &CoinWatchlistCache,
    market_id: &str,
    offers: &[Value],
) -> SignerResult<()> {
    let watch_offer_ids = watchlist_offer_ids_for_coin_tracking(store, market_id)?;
    let mut watched_coin_ids: HashSet<String> = HashSet::default();
    let mut matched_offer_count = 0_u64;
    for offer in offers {
        let offer_id = offer.get("id").and_then(Value::as_str).unwrap_or("").trim();
        if offer_id.is_empty() || !watch_offer_ids.contains(offer_id) {
            continue;
        }
        matched_offer_count += 1;
        for coin_id in extract_coin_ids_from_offer_payload(offer) {
            watched_coin_ids.insert(coin_id);
        }
    }
    set_watched_coin_ids_for_market(cache, market_id, watched_coin_ids.clone());
    let mut sample: Vec<String> = watched_coin_ids.iter().cloned().collect();
    sample.sort();
    sample.truncate(10);
    store.add_audit_event(
        "coin_watchlist_updated",
        &serde_json::json!({
            "market_id": market_id,
            "watch_offer_count": watch_offer_ids.len(),
            "matched_offer_count": matched_offer_count,
            "watch_coin_count": watched_coin_ids.len(),
            "watch_coin_sample": sample,
        }),
        Some(market_id),
    )
}

#[cfg(test)]
mod tests;

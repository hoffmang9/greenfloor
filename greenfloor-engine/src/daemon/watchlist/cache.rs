use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Per-daemon coin IDs watched for websocket hit detection (updated each reconcile).
#[derive(Debug, Default)]
pub struct CoinWatchlistCache {
    inner: Mutex<HashMap<String, HashSet<String>>>,
}

impl CoinWatchlistCache {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn watched_coin_ids_for_market(&self, market_id: &str) -> HashSet<String> {
        let key = market_id.trim();
        if key.is_empty() {
            return HashSet::new();
        }
        let Ok(cache) = self.inner.lock() else {
            return HashSet::new();
        };
        cache.get(key).cloned().unwrap_or_default()
    }

    pub fn set_watched_coin_ids_for_market(&self, market_id: &str, coin_ids: HashSet<String>) {
        let key = market_id.trim();
        if key.is_empty() {
            return;
        }
        let normalized: HashSet<String> = coin_ids
            .into_iter()
            .map(|coin_id| coin_id.trim().to_ascii_lowercase())
            .filter(|coin_id| !coin_id.is_empty())
            .collect();
        if let Ok(mut cache) = self.inner.lock() {
            cache.insert(key.to_string(), normalized);
        }
    }

    pub fn match_watched_coin_ids(
        &self,
        observed_coin_ids: &[String],
    ) -> HashMap<String, Vec<String>> {
        let normalized: HashSet<String> = observed_coin_ids
            .iter()
            .map(|coin_id| coin_id.trim().to_ascii_lowercase())
            .filter(|coin_id| !coin_id.is_empty())
            .collect();
        if normalized.is_empty() {
            return HashMap::new();
        }
        let Ok(cache) = self.inner.lock() else {
            return HashMap::new();
        };
        let mut matches = HashMap::new();
        for (market_id, watched) in cache.iter() {
            let mut intersection: Vec<String> = normalized.intersection(watched).cloned().collect();
            if intersection.is_empty() {
                continue;
            }
            intersection.sort();
            matches.insert(market_id.clone(), intersection);
        }
        matches
    }
}

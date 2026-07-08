//! Inventory freshness driven by Coinset WS p2 hits.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Skip Coinset inventory HTTP polls when no relevant WS activity within this window.
pub const INVENTORY_MAX_STALENESS: Duration = Duration::from_secs(90);

#[derive(Debug, Default)]
struct InventoryFreshnessInner {
    /// `market_id` -> last successful HTTP inventory bucket counts.
    last_buckets: HashMap<String, BTreeMap<i64, i64>>,
    /// `market_id` -> last time inventory was considered fresh.
    last_fresh_at: HashMap<String, Instant>,
    /// `market_id` -> marked stale by WS `p2`/coin hit.
    stale: HashMap<String, bool>,
}

/// Process-wide inventory freshness tracker shared by WS handler and inventory phase.
#[derive(Debug, Default)]
pub struct InventoryFreshnessCache {
    inner: Mutex<InventoryFreshnessInner>,
}

impl InventoryFreshnessCache {
    #[must_use]
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self::default())
    }

    pub fn mark_stale(&self, market_id: &str) {
        let clean = market_id.trim();
        if clean.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.inner.lock() {
            guard.stale.insert(clean.to_string(), true);
        }
    }

    pub fn mark_fresh(&self, market_id: &str, buckets: BTreeMap<i64, i64>) {
        let clean = market_id.trim();
        if clean.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.inner.lock() {
            guard.stale.insert(clean.to_string(), false);
            guard
                .last_fresh_at
                .insert(clean.to_string(), Instant::now());
            guard.last_buckets.insert(clean.to_string(), buckets);
        }
    }

    /// Cached bucket counts from the last successful HTTP refresh, if any.
    #[must_use]
    pub fn cached_buckets(&self, market_id: &str) -> Option<BTreeMap<i64, i64>> {
        let clean = market_id.trim();
        if clean.is_empty() {
            return None;
        }
        let Ok(guard) = self.inner.lock() else {
            return None;
        };
        guard.last_buckets.get(clean).cloned()
    }

    /// Whether inventory should be refreshed via HTTP for this market.
    #[must_use]
    pub fn needs_refresh(&self, market_id: &str, max_staleness: Duration) -> bool {
        let clean = market_id.trim();
        if clean.is_empty() {
            return true;
        }
        let Ok(guard) = self.inner.lock() else {
            return true;
        };
        if guard.stale.get(clean).copied().unwrap_or(true) {
            return true;
        }
        if !guard.last_buckets.contains_key(clean) {
            return true;
        }
        match guard.last_fresh_at.get(clean) {
            Some(at) => at.elapsed() >= max_staleness,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn needs_refresh_when_stale_or_never_fresh() {
        let cache = InventoryFreshnessCache::new();
        assert!(cache.needs_refresh("m1", INVENTORY_MAX_STALENESS));
        let mut buckets = BTreeMap::new();
        buckets.insert(1, 2);
        cache.mark_fresh("m1", buckets.clone());
        assert!(!cache.needs_refresh("m1", Duration::from_secs(60)));
        assert_eq!(cache.cached_buckets("m1"), Some(buckets));
        cache.mark_stale("m1");
        assert!(cache.needs_refresh("m1", Duration::from_secs(60)));
    }

    #[test]
    fn max_staleness_forces_refresh() {
        let cache = InventoryFreshnessCache::new();
        cache.mark_fresh("m1", BTreeMap::from([(10, 1)]));
        thread::sleep(Duration::from_millis(20));
        assert!(cache.needs_refresh("m1", Duration::from_millis(5)));
    }
}

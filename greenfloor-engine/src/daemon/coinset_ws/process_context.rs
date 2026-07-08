//! Process-scoped Coinset WS context: inventory p2 index + freshness.

use std::sync::Arc;

use super::p2_filters::InventoryP2Index;
use crate::daemon::inventory_freshness::InventoryFreshnessCache;

/// Shared process context for Coinset WS filters, freshness, and inventory skip.
#[derive(Debug, Clone)]
pub struct CoinsetProcessContext {
    pub inventory_p2s: Arc<InventoryP2Index>,
    pub inventory_freshness: Arc<InventoryFreshnessCache>,
}

impl CoinsetProcessContext {
    #[must_use]
    pub fn new(
        inventory_p2s: Arc<InventoryP2Index>,
        inventory_freshness: Arc<InventoryFreshnessCache>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inventory_p2s,
            inventory_freshness,
        })
    }

    #[must_use]
    pub fn empty() -> Arc<Self> {
        Self::new(
            Arc::new(InventoryP2Index::default()),
            InventoryFreshnessCache::new(),
        )
    }
}

impl Default for CoinsetProcessContext {
    fn default() -> Self {
        Self {
            inventory_p2s: Arc::new(InventoryP2Index::default()),
            inventory_freshness: InventoryFreshnessCache::new(),
        }
    }
}

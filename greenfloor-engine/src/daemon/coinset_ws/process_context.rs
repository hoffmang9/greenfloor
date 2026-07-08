//! Process-scoped Coinset WS context: inventory p2 index + freshness.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use super::p2_filters::InventoryP2Index;
use crate::daemon::inventory_freshness::InventoryFreshnessCache;
use crate::error::SignerResult;

/// Shared process context for Coinset WS filters, freshness, and inventory skip.
#[derive(Debug)]
pub struct CoinsetProcessContext {
    inventory_p2s: RwLock<Arc<InventoryP2Index>>,
    pub inventory_freshness: Arc<InventoryFreshnessCache>,
    /// Set when markets reload; WS loop breaks the current connection to rebuild URL filters.
    reconnect_requested: AtomicBool,
}

impl CoinsetProcessContext {
    #[must_use]
    pub fn new(
        inventory_p2s: Arc<InventoryP2Index>,
        inventory_freshness: Arc<InventoryFreshnessCache>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inventory_p2s: RwLock::new(inventory_p2s),
            inventory_freshness,
            reconnect_requested: AtomicBool::new(false),
        })
    }

    /// Build process context from enabled markets (same path as the daemon loop).
    ///
    /// # Errors
    ///
    /// Returns an error if markets cannot be loaded or inventory p2s cannot be derived.
    pub fn from_markets(
        markets_path: &Path,
        testnet_markets_path: Option<&Path>,
    ) -> SignerResult<Arc<Self>> {
        let inventory_p2s = InventoryP2Index::from_markets(markets_path, testnet_markets_path)?;
        Ok(Self::new(inventory_p2s, InventoryFreshnessCache::new()))
    }

    #[must_use]
    pub fn empty() -> Arc<Self> {
        Self::new(
            Arc::new(InventoryP2Index::default()),
            InventoryFreshnessCache::new(),
        )
    }

    /// Current inventory p2 index (clone of the `Arc`).
    #[must_use]
    pub fn inventory_p2s(&self) -> Arc<InventoryP2Index> {
        self.inventory_p2s
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Replace inventory p2 filters after a markets config reload.
    pub fn replace_inventory_p2s(&self, inventory_p2s: Arc<InventoryP2Index>) {
        *self
            .inventory_p2s
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = inventory_p2s;
    }

    /// Ask the background WS loop to drop the current connection and reconnect
    /// with the current inventory p2 filters.
    pub fn request_ws_reconnect(&self) {
        self.reconnect_requested.store(true, Ordering::SeqCst);
    }

    /// Consume a pending reconnect request (WS loop).
    #[must_use]
    pub fn take_ws_reconnect_requested(&self) -> bool {
        self.reconnect_requested.swap(false, Ordering::SeqCst)
    }
}

impl Default for CoinsetProcessContext {
    fn default() -> Self {
        Self {
            inventory_p2s: RwLock::new(Arc::new(InventoryP2Index::default())),
            inventory_freshness: InventoryFreshnessCache::new(),
            reconnect_requested: AtomicBool::new(false),
        }
    }
}

impl Clone for CoinsetProcessContext {
    fn clone(&self) -> Self {
        Self {
            inventory_p2s: RwLock::new(self.inventory_p2s()),
            inventory_freshness: Arc::clone(&self.inventory_freshness),
            reconnect_requested: AtomicBool::new(self.reconnect_requested.load(Ordering::SeqCst)),
        }
    }
}

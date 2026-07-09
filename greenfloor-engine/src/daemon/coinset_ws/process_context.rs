//! Shared Coinset WS handles: inventory p2 index, freshness, reconnect flag.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use super::p2_filters::InventoryP2Index;
use crate::daemon::inventory_freshness::InventoryFreshnessCache;
use crate::error::SignerResult;

/// Process-scoped handles for Coinset WS filters, inventory freshness, and reconnect.
///
/// Prefer reading fields directly (`inventory_freshness`, `inventory_p2s`) over wrapping
/// more behavior here. Share via `Arc`.
#[derive(Debug)]
pub struct CoinsetWsShared {
    pub inventory_p2s: RwLock<Arc<InventoryP2Index>>,
    pub inventory_freshness: Arc<InventoryFreshnessCache>,
    /// Set when markets reload; WS loop breaks the current connection to rebuild URL filters.
    pub reconnect_requested: AtomicBool,
}

impl CoinsetWsShared {
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

    /// Build from enabled markets (same path as the daemon loop).
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

    /// Build from markets, or empty filters with a warning on total failure.
    ///
    /// Per-market skip already happens inside [`InventoryP2Index::from_markets`].
    #[must_use]
    pub fn from_markets_or_empty(
        markets_path: &Path,
        testnet_markets_path: Option<&Path>,
    ) -> Arc<Self> {
        match Self::from_markets(markets_path, testnet_markets_path) {
            Ok(ctx) => ctx,
            Err(err) => {
                tracing::warn!(
                    markets_path = %markets_path.display(),
                    error = %err,
                    "inventory p2 index build failed; continuing with empty filters"
                );
                Self::empty()
            }
        }
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
    pub fn p2_index(&self) -> Arc<InventoryP2Index> {
        self.inventory_p2s
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Replace inventory p2 filters after a markets config reload.
    pub fn replace_p2_index(&self, inventory_p2s: Arc<InventoryP2Index>) {
        *self
            .inventory_p2s
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = inventory_p2s;
    }

    /// Ask the background WS loop to drop the current connection and reconnect.
    pub fn request_reconnect(&self) {
        self.reconnect_requested.store(true, Ordering::SeqCst);
    }

    /// Consume a pending reconnect request (WS loop).
    #[must_use]
    pub fn take_reconnect_requested(&self) -> bool {
        self.reconnect_requested.swap(false, Ordering::SeqCst)
    }
}

//! Stable inventory puzzle hashes for Coinset WS filters and freshness.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use crate::coinset::market_inventory_p2s;
use crate::config::{
    load_markets_config_with_overlay, lookup_asset_id_from_ticker, operator_ticker_index_from_paths,
};
use crate::error::SignerResult;
use crate::offer::assets::normalize_asset_id;

/// Process-wide inventory p2 set plus reverse map for freshness invalidation.
#[derive(Debug, Clone, Default)]
pub struct InventoryP2Index {
    p2s: Vec<String>,
    markets_by_p2: HashMap<String, Vec<String>>,
}

impl InventoryP2Index {
    /// Build from enabled markets (receive puzzle + CAT outer hash).
    ///
    /// # Errors
    ///
    /// Returns an error if markets/cats cannot be loaded, an address cannot be decoded,
    /// or a market base asset cannot be resolved to a CAT id when needed.
    pub fn from_markets(
        markets_path: &Path,
        testnet_markets_path: Option<&Path>,
    ) -> SignerResult<Arc<Self>> {
        let markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
        let ticker_index =
            operator_ticker_index_from_paths(markets_path, testnet_markets_path, None);
        let mut markets_by_p2: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for market in markets.markets.iter().filter(|m| m.enabled) {
            let receive = market.receive_address.trim();
            if receive.is_empty() {
                tracing::warn!(
                    market_id = %market.market_id,
                    "skipping market for coinset ws p2 filters: missing receive_address"
                );
                continue;
            }
            let base = market.base_asset.trim();
            let base_asset_id = if base.is_empty()
                || base.eq_ignore_ascii_case("xch")
                || base.eq_ignore_ascii_case("txch")
            {
                None
            } else if let Ok(normalized) = normalize_asset_id(base) {
                Some(normalized)
            } else if let Some(resolved) = lookup_asset_id_from_ticker(&ticker_index, base)? {
                Some(normalize_asset_id(&resolved)?)
            } else {
                tracing::warn!(
                    market_id = %market.market_id,
                    base_asset = %base,
                    "skipping market for coinset ws p2 filters: base_asset could not be resolved"
                );
                continue;
            };
            for p2 in market_inventory_p2s(receive, base_asset_id.as_deref())? {
                markets_by_p2
                    .entry(p2)
                    .or_default()
                    .insert(market.market_id.clone());
            }
        }
        let p2s: Vec<String> = markets_by_p2.keys().cloned().collect();
        let markets_by_p2 = markets_by_p2
            .into_iter()
            .map(|(p2, markets)| (p2, markets.into_iter().collect()))
            .collect();
        Ok(Arc::new(Self { p2s, markets_by_p2 }))
    }

    #[must_use]
    pub fn p2s(&self) -> &[String] {
        &self.p2s
    }

    /// Test/helper constructor from an explicit p2 → markets map.
    #[must_use]
    pub fn from_markets_by_p2(markets_by_p2: HashMap<String, Vec<String>>) -> Arc<Self> {
        let p2s: Vec<String> = markets_by_p2.keys().cloned().collect();
        Arc::new(Self { p2s, markets_by_p2 })
    }

    /// Market ids whose inventory p2s intersect `observed_p2s`.
    #[must_use]
    pub fn market_ids_for_p2s(&self, observed_p2s: &[String]) -> Vec<String> {
        let mut markets = BTreeSet::new();
        for p2 in observed_p2s {
            let normalized = crate::hex::normalize_hex_id(p2);
            if let Some(ids) = self.markets_by_p2.get(&normalized) {
                markets.extend(ids.iter().cloned());
            }
        }
        markets.into_iter().collect()
    }

    /// Inventory p2s registered for one market id.
    #[must_use]
    pub fn p2s_for_market(&self, market_id: &str) -> Vec<String> {
        let clean = market_id.trim();
        if clean.is_empty() {
            return Vec::new();
        }
        let mut p2s: Vec<String> = self
            .markets_by_p2
            .iter()
            .filter(|(_, markets)| markets.iter().any(|id| id == clean))
            .map(|(p2, _)| p2.clone())
            .collect();
        p2s.sort();
        p2s
    }
}

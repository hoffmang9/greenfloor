//! Stable inventory puzzle hashes for Coinset WS filters and freshness.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use crate::coinset::{cat_outer_puzzle_hash_hex, puzzle_hash_hex_for_receive_address};
use crate::config::{
    load_markets_config_with_overlay, lookup_asset_id_from_ticker, operator_ticker_index_from_paths,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
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
                return Err(SignerError::Other(format!(
                    "market {} missing receive_address for coinset ws p2 filters",
                    market.market_id
                )));
            }
            let inner = normalize_hex_id(&puzzle_hash_hex_for_receive_address(receive)?);
            if inner.len() == 64 {
                markets_by_p2
                    .entry(inner)
                    .or_default()
                    .insert(market.market_id.clone());
            }

            let base = market.base_asset.trim();
            if base.is_empty()
                || base.eq_ignore_ascii_case("xch")
                || base.eq_ignore_ascii_case("txch")
            {
                continue;
            }
            let asset_id = if let Ok(normalized) = normalize_asset_id(base) {
                normalized
            } else if let Some(resolved) = lookup_asset_id_from_ticker(&ticker_index, base)? {
                normalize_asset_id(&resolved)?
            } else {
                return Err(SignerError::Other(format!(
                    "market {} base_asset `{base}` could not be resolved to a CAT asset id for ws p2 filters",
                    market.market_id
                )));
            };
            let outer = normalize_hex_id(&cat_outer_puzzle_hash_hex(receive, &asset_id)?);
            if outer.len() == 64 {
                markets_by_p2
                    .entry(outer)
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
            let normalized = normalize_hex_id(p2);
            if let Some(ids) = self.markets_by_p2.get(&normalized) {
                markets.extend(ids.iter().cloned());
            }
        }
        markets.into_iter().collect()
    }
}

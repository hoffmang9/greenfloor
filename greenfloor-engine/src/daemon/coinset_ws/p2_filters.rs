//! Stable inventory puzzle hashes for Coinset WS filters and freshness.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use crate::coinset::market_inventory_p2s;
use crate::config::{
    load_markets_config_with_overlay, lookup_asset_id_from_ticker,
    operator_ticker_index_from_paths, CatTickerIndex, MarketConfig,
};
use crate::error::SignerResult;
use crate::offer::assets::normalize_asset_id;

/// Process-wide inventory p2 set plus reverse map for freshness invalidation.
#[derive(Debug, Clone, Default)]
pub struct InventoryP2Index {
    p2s: Vec<String>,
    markets_by_p2: HashMap<String, Vec<String>>,
}

/// Resolve a market base asset to a CAT id, or `None` for XCH-like markets.
///
/// Returns `Err(reason)` when the market should be skipped for inventory p2 filters.
fn resolve_market_base_asset_id(
    market: &MarketConfig,
    ticker_index: &CatTickerIndex,
) -> Result<Option<String>, String> {
    let base = market.base_asset.trim();
    if base.is_empty() || base.eq_ignore_ascii_case("xch") || base.eq_ignore_ascii_case("txch") {
        return Ok(None);
    }
    if let Ok(normalized) = normalize_asset_id(base) {
        return Ok(Some(normalized));
    }
    match lookup_asset_id_from_ticker(ticker_index, base) {
        Ok(Some(resolved)) => normalize_asset_id(&resolved)
            .map(Some)
            .map_err(|err| format!("resolved base_asset normalize failed: {err}")),
        Ok(None) => Err("base_asset could not be resolved".to_string()),
        Err(err) => Err(format!("base_asset ticker lookup failed: {err}")),
    }
}

impl InventoryP2Index {
    /// Build from enabled markets (receive puzzle + CAT outer hash).
    ///
    /// Per-market failures (bad receive address, unresolved/ambiguous base asset,
    /// inventory p2 derivation) are skipped with a warning so one bad market does
    /// not empty filters for every market.
    ///
    /// # Errors
    ///
    /// Returns an error if markets/cats cannot be loaded.
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
            let base_asset_id = match resolve_market_base_asset_id(market, &ticker_index) {
                Ok(id) => id,
                Err(reason) => {
                    tracing::warn!(
                        market_id = %market.market_id,
                        base_asset = %market.base_asset.trim(),
                        reason = %reason,
                        "skipping market for coinset ws p2 filters"
                    );
                    continue;
                }
            };
            let Ok(p2s) = market_inventory_p2s(receive, base_asset_id.as_deref()) else {
                tracing::warn!(
                    market_id = %market.market_id,
                    receive_address = %receive,
                    "skipping market for coinset ws p2 filters: inventory p2 derivation failed"
                );
                continue;
            };
            for p2 in p2s {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bech32m::encode_address;
    use chia_protocol::Bytes32;
    use std::fs;
    use tempfile::tempdir;

    fn write_markets_yaml(path: &std::path::Path, body: &str) {
        fs::write(path, body).expect("write markets");
    }

    #[test]
    fn from_markets_skips_bad_market_and_keeps_good_p2s() {
        let dir = tempdir().expect("tempdir");
        let markets_path = dir.path().join("markets.yaml");
        let good_receive = encode_address(Bytes32::new([0x11; 32]), "xch").expect("encode receive");
        write_markets_yaml(
            &markets_path,
            &format!(
                r#"
markets:
  - id: good
    enabled: true
    base_asset: xch
    base_symbol: XCH
    quote_asset: xch
    quote_asset_type: unstable
    receive_address: "{good_receive}"
    signer_key_id: key-1
    mode: sell_only
    pricing:
      quote_price: 1.0
  - id: bad-receive
    enabled: true
    base_asset: xch
    base_symbol: XCH
    quote_asset: xch
    quote_asset_type: unstable
    receive_address: "not-a-valid-address"
    signer_key_id: key-1
    mode: sell_only
    pricing:
      quote_price: 1.0
  - id: bad-unresolved
    enabled: true
    base_asset: NOSUCHTICKER
    base_symbol: NOSUCH
    quote_asset: xch
    quote_asset_type: unstable
    receive_address: "{good_receive}"
    signer_key_id: key-1
    mode: sell_only
    pricing:
      quote_price: 1.0
"#
            ),
        );

        let index = InventoryP2Index::from_markets(&markets_path, None).expect("index");
        let expected_p2 = crate::hex::normalize_hex_id(&hex::encode([0x11u8; 32]));
        assert_eq!(index.p2s(), std::slice::from_ref(&expected_p2));
        assert_eq!(index.p2s_for_market("good"), vec![expected_p2]);
        assert!(index.p2s_for_market("bad-receive").is_empty());
        assert!(index.p2s_for_market("bad-unresolved").is_empty());
    }
}

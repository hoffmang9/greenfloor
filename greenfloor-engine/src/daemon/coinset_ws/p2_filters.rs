//! Collect stable maker/inventory puzzle hashes for Coinset WS `p2` filters.

use std::collections::BTreeSet;
use std::path::Path;

use crate::coinset::{cat_outer_puzzle_hash_hex, puzzle_hash_hex_for_receive_address};
use crate::config::{
    load_markets_config_with_overlay, lookup_asset_id_from_ticker, operator_ticker_index_from_paths,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::offer::assets::normalize_asset_id;

/// Build the stable p2 set from enabled markets (receive puzzle + CAT outer hash).
///
/// CAT asset ids come from hex `base_asset` or the operator cats ticker index.
///
/// # Errors
///
/// Returns an error if markets/cats cannot be loaded, an address cannot be decoded,
/// or a market base asset cannot be resolved to a CAT id when needed.
pub fn stable_inventory_p2s_from_markets(
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
) -> SignerResult<Vec<String>> {
    let markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
    let ticker_index = operator_ticker_index_from_paths(markets_path, testnet_markets_path, None);
    let mut p2s = BTreeSet::new();
    for market in markets.markets.iter().filter(|m| m.enabled) {
        let receive = market.receive_address.trim();
        if receive.is_empty() {
            return Err(SignerError::Other(format!(
                "market {} missing receive_address for coinset ws p2 filters",
                market.market_id
            )));
        }
        let inner = puzzle_hash_hex_for_receive_address(receive)?;
        p2s.insert(normalize_hex_id(&inner));

        let base = market.base_asset.trim();
        if base.is_empty() || base.eq_ignore_ascii_case("xch") || base.eq_ignore_ascii_case("txch")
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
        let outer = cat_outer_puzzle_hash_hex(receive, &asset_id)?;
        p2s.insert(normalize_hex_id(&outer));
    }
    Ok(p2s.into_iter().filter(|p2| p2.len() == 64).collect())
}

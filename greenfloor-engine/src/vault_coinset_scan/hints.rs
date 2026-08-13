//! CAT receive-address hints for vault Coinset discovery.

use crate::coinset::puzzle_hash_hex_for_receive_address;
use crate::config::{CatTickerIndex, MarketConfig};
use crate::error::SignerResult;
use crate::hex::normalize_hex_id;

fn market_matches_cat_asset(
    ticker_index: &CatTickerIndex,
    market: &MarketConfig,
    resolved_asset_id: &str,
    requested_asset: &str,
) -> bool {
    let requested = requested_asset.trim().to_ascii_lowercase();
    for label in [market.base_asset.as_str(), market.base_symbol.as_str()] {
        if label.trim().to_ascii_lowercase() == requested {
            return true;
        }
        if ticker_index.label_refers_to_asset(label, resolved_asset_id) {
            return true;
        }
    }
    false
}

/// Collect unique receive p2 hashes for markets whose base matches the CAT asset.
///
/// # Errors
///
/// Returns an error if a matching market's receive address cannot be decoded.
pub fn cat_receive_hint_puzzle_hashes(
    markets: &[MarketConfig],
    ticker_index: &CatTickerIndex,
    resolved_asset_id: &str,
    requested_asset: &str,
) -> SignerResult<Vec<String>> {
    let resolved = normalize_hex_id(resolved_asset_id);
    let mut hashes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for market in markets {
        if !market_matches_cat_asset(ticker_index, market, &resolved, requested_asset)
            || market.receive_address.trim().is_empty()
        {
            continue;
        }
        let hash = normalize_hex_id(&puzzle_hash_hex_for_receive_address(
            &market.receive_address,
        )?);
        if !hash.is_empty() && seen.insert(hash.clone()) {
            hashes.push(hash);
        }
    }
    Ok(hashes)
}

#[cfg(test)]
mod tests {
    use super::{cat_receive_hint_puzzle_hashes, market_matches_cat_asset};
    use crate::coinset::puzzle_hash_hex_for_receive_address;
    use crate::config::{CatTickerIndex, MarketConfig};
    use crate::hex::normalize_hex_id;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn market_matches_cat_asset_via_base_symbol() {
        let asset_id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let mut by_ticker = HashMap::new();
        by_ticker.insert("byc".to_string(), HashSet::from([asset_id.to_string()]));
        let index = CatTickerIndex {
            by_ticker,
            symbols_by_asset_id: std::collections::BTreeMap::default(),
        };
        let market = MarketConfig {
            market_id: "m".to_string(),
            enabled: true,
            unique_maker_coins: true,
            base_asset: "unrelated".to_string(),
            base_symbol: "BYC".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "volatile".to_string(),
            receive_address: String::new(),
            signer_key_id: "k".to_string(),
            mode: "one_sided".to_string(),
            pricing: crate::config::MarketPricing::default(),
            cancel_move_threshold_bps: None,
            ladders: HashMap::new(),
        };
        assert!(market_matches_cat_asset(&index, &market, asset_id, "other"));
    }

    #[test]
    fn cat_receive_hints_match_ticker_market_when_operator_passes_asset_id() {
        let asset_id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let mut by_ticker = HashMap::new();
        by_ticker.insert("byc".to_string(), HashSet::from([asset_id.to_string()]));
        let index = CatTickerIndex {
            by_ticker,
            symbols_by_asset_id: std::collections::BTreeMap::default(),
        };
        let receive = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h".to_string();
        let expected =
            normalize_hex_id(&puzzle_hash_hex_for_receive_address(&receive).expect("receive p2"));
        let market = MarketConfig {
            market_id: "byc-xch".to_string(),
            enabled: true,
            unique_maker_coins: true,
            base_asset: "BYC".to_string(),
            base_symbol: "BYC".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "volatile".to_string(),
            receive_address: receive,
            signer_key_id: "k".to_string(),
            mode: "one_sided".to_string(),
            pricing: crate::config::MarketPricing::default(),
            cancel_move_threshold_bps: None,
            ladders: HashMap::new(),
        };

        let hashes =
            cat_receive_hint_puzzle_hashes(&[market], &index, asset_id, asset_id).expect("hints");
        assert_eq!(hashes, vec![expected]);
    }
}

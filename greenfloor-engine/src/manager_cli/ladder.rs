//! Sell-ladder resolution for manager coin-op commands.

use crate::config::{LadderEntry, MarketConfig};
use crate::error::{SignerError, SignerResult};

pub fn sell_ladder_entry_for_size(
    market: &MarketConfig,
    size_base_units: i64,
) -> SignerResult<&LadderEntry> {
    let ladder = market.ladders.get("sell").ok_or_else(|| {
        SignerError::Other(format!(
            "market {} has no sell ladder; cannot resolve denomination target",
            market.market_id
        ))
    })?;
    ladder
        .iter()
        .find(|row| row.size_base_units == size_base_units)
        .ok_or_else(|| {
            let allowed = ladder
                .iter()
                .map(|row| row.size_base_units.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            SignerError::Other(format!(
                "size_base_units not configured for market sell ladder; use one of: {allowed}"
            ))
        })
}

pub fn resolve_split_targets(
    market: &MarketConfig,
    amount_per_coin: i64,
    number_of_coins: i64,
    size_base_units: Option<i64>,
) -> SignerResult<(i64, i64)> {
    if let Some(size) = size_base_units.filter(|value| *value > 0) {
        let entry = sell_ladder_entry_for_size(market, size)?;
        let required_count = entry.target_count + entry.split_buffer_count;
        let amount = if amount_per_coin > 0 {
            amount_per_coin
        } else {
            entry.size_base_units
        };
        let count = if number_of_coins > 0 {
            number_of_coins
        } else {
            required_count
        };
        return Ok((amount, count));
    }
    Ok((amount_per_coin, number_of_coins))
}

pub fn resolve_combine_count(
    market: &MarketConfig,
    number_of_coins: i64,
    size_base_units: Option<i64>,
) -> SignerResult<i64> {
    if let Some(size) = size_base_units.filter(|value| *value > 0) {
        let entry = sell_ladder_entry_for_size(market, size)?;
        let threshold =
            ((entry.target_count as f64) * entry.combine_when_excess_factor).ceil() as i64;
        let count = if number_of_coins > 0 {
            number_of_coins
        } else {
            threshold.max(2)
        };
        return Ok(count);
    }
    Ok(number_of_coins)
}

pub fn split_required_count(entry: &LadderEntry) -> i64 {
    entry.target_count + entry.split_buffer_count
}

pub fn combine_threshold_count(entry: &LadderEntry) -> i64 {
    ((entry.target_count as f64) * entry.combine_when_excess_factor)
        .ceil()
        .max(2.0) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_market() -> MarketConfig {
        MarketConfig {
            market_id: "m1".to_string(),
            enabled: true,
            base_asset: "xch".to_string(),
            base_symbol: "XCH".to_string(),
            quote_asset: "usd".to_string(),
            quote_asset_type: "stable".to_string(),
            receive_address: "xch1test".to_string(),
            signer_key_id: "key-1".to_string(),
            mode: "sell_only".to_string(),
            pricing: serde_json::json!({}),
            cancel_move_threshold_bps: None,
            ladders: HashMap::from([(
                "sell".to_string(),
                vec![LadderEntry {
                    size_base_units: 100,
                    target_count: 2,
                    split_buffer_count: 1,
                    combine_when_excess_factor: 1.5,
                }],
            )]),
        }
    }

    #[test]
    fn resolve_split_targets_from_ladder_size() {
        let market = sample_market();
        let (amount, count) = resolve_split_targets(&market, 0, 0, Some(100)).expect("ladder row");
        assert_eq!(amount, 100);
        assert_eq!(count, 3);
    }

    #[test]
    fn resolve_combine_count_from_ladder_size() {
        let market = sample_market();
        let count = resolve_combine_count(&market, 0, Some(100)).expect("ladder row");
        assert_eq!(count, 3);
    }

    #[test]
    fn combine_threshold_count_uses_ceil() {
        let entry = LadderEntry {
            size_base_units: 10,
            target_count: 3,
            split_buffer_count: 1,
            combine_when_excess_factor: 1.5,
        };
        assert_eq!(combine_threshold_count(&entry), 5);
        let mut market = sample_market();
        market
            .ladders
            .insert("sell".to_string(), vec![entry.clone()]);
        let count = resolve_combine_count(&market, 0, Some(10)).expect("ladder row");
        assert_eq!(count, 5);
    }
}

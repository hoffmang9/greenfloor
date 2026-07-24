use super::{MarketConfig, MarketsConfig};
use crate::config::program::is_testnet_network;
use crate::error::{SignerError, SignerResult};

/// Which market-resolution rules apply when loading a single operator market row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorMarketCommand {
    /// `build-and-post-offer`, coin-split/combine: require exactly one of `--market-id` or `--pair`.
    Build,
    /// `coins-list` / `coin-status`: allow default market for the operator network.
    CoinList,
}

/// Canonical market resolution for operator commands: select, then enforce network match.
///
/// # Errors
///
/// Returns an error if selection fails or the receive address mismatches the network.
pub fn resolve_operator_market(
    markets: &MarketsConfig,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    command: OperatorMarketCommand,
) -> SignerResult<MarketConfig> {
    let market = match command {
        OperatorMarketCommand::Build => {
            resolve_market_for_build(markets, market_id, pair, network)?
        }
        OperatorMarketCommand::CoinList => {
            select_coin_list_market(markets, network, market_id, pair)?
        }
    };
    ensure_market_receive_address_for_network(&market, network)?;
    Ok(market)
}

/// Resolve by exactly one of `--market-id` or `--pair` (no network enforcement).
///
/// # Errors
///
/// Returns an error if args are invalid, no market matches, or the pair is ambiguous.
pub fn resolve_market_for_build(
    markets: &MarketsConfig,
    market_id: Option<&str>,
    pair: Option<&str>,
    network: &str,
) -> SignerResult<MarketConfig> {
    match (nonempty(market_id), nonempty(pair)) {
        (Some(market_id), None) => markets
            .markets
            .iter()
            .find(|market| market.market_id == market_id)
            .cloned()
            .ok_or_else(|| SignerError::Other(format!("market_id not found: {market_id}"))),
        (None, Some(pair)) => resolve_by_pair(markets, pair, network),
        _ => Err(SignerError::Other(
            "provide exactly one of --market-id or --pair".to_string(),
        )),
    }
}

/// Require a market row's receive address to match the operator network.
///
/// # Errors
///
/// Returns an error when the address prefix does not match the network.
pub fn ensure_market_receive_address_for_network(
    market: &MarketConfig,
    network: &str,
) -> SignerResult<()> {
    if receive_address_matches_operator_network(&market.receive_address, network) {
        return Ok(());
    }
    Err(SignerError::Other(format!(
        "market {} receive_address does not match operator network {network}",
        market.market_id
    )))
}

/// Whether a receive address belongs on the given operator network.
#[must_use]
pub fn receive_address_matches_operator_network(receive_address: &str, network: &str) -> bool {
    let addr = receive_address.trim().to_ascii_lowercase();
    if is_testnet_network(network) {
        addr.starts_with("txch1")
    } else {
        addr.starts_with("xch1")
    }
}

fn select_coin_list_market(
    markets: &MarketsConfig,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
) -> SignerResult<MarketConfig> {
    if nonempty(market_id).is_some() || nonempty(pair).is_some() {
        return resolve_market_for_build(markets, market_id, pair, network);
    }
    markets
        .markets
        .iter()
        .filter(|market| {
            market.enabled
                && receive_address_matches_operator_network(&market.receive_address, network)
        })
        .min_by_key(|market| market.market_id.as_str())
        .cloned()
        .ok_or_else(|| {
            SignerError::Other(format!(
                "no enabled market with receive_address for network {network}"
            ))
        })
}

fn resolve_by_pair(
    markets: &MarketsConfig,
    pair: &str,
    network: &str,
) -> SignerResult<MarketConfig> {
    let (base, quote) = parse_pair(pair)?;
    let mut matches = markets
        .markets
        .iter()
        .filter(|market| market_matches_pair(market, &base, &quote, network));
    match (matches.next(), matches.next()) {
        (None, _) => Err(SignerError::Other(format!(
            "no enabled market found for pair: {pair}"
        ))),
        (Some(only), None) => Ok(only.clone()),
        (Some(first), Some(second)) => {
            let mut ids = vec![first.market_id.as_str(), second.market_id.as_str()];
            ids.extend(matches.map(|market| market.market_id.as_str()));
            Err(SignerError::Other(format!(
                "pair is ambiguous; use --market-id (candidates: {})",
                ids.join(", ")
            )))
        }
    }
}

fn market_matches_pair(market: &MarketConfig, base: &str, quote: &str, network: &str) -> bool {
    market.enabled
        && [market.base_asset.as_str(), market.base_symbol.as_str()]
            .iter()
            .any(|value| value.trim().eq_ignore_ascii_case(base))
        && quote_asset_matches(&market.quote_asset, quote, network)
}

fn quote_asset_matches(market_quote: &str, wanted: &str, network: &str) -> bool {
    let mq = market_quote.trim().to_ascii_lowercase();
    mq == wanted
        || (is_testnet_network(network)
            && matches!((mq.as_str(), wanted), ("xch", "txch") | ("txch", "xch")))
}

fn parse_pair(pair: &str) -> SignerResult<(String, String)> {
    let (base, quote) = pair
        .split_once(':')
        .or_else(|| pair.split_once('/'))
        .ok_or_else(|| {
            SignerError::Other("pair must be in base:quote or base/quote format".to_string())
        })?;
    let base = base.trim();
    let quote = quote.trim();
    if base.is_empty() {
        return Err(SignerError::Other(
            "pair base must be non-empty".to_string(),
        ));
    }
    if quote.is_empty() {
        return Err(SignerError::Other(
            "pair quote must be non-empty".to_string(),
        ));
    }
    Ok((base.to_ascii_lowercase(), quote.to_ascii_lowercase()))
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

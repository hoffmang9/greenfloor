//! Shared manager CLI helpers.

use crate::error::{SignerError, SignerResult};

pub fn require_market_selector(market_id: Option<&str>, pair: Option<&str>) -> SignerResult<()> {
    let has_market_id = market_id
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let has_pair = pair.map(str::trim).is_some_and(|value| !value.is_empty());
    if has_market_id == has_pair {
        return Err(SignerError::Other(
            "provide exactly one of --market-id or --pair".to_string(),
        ));
    }
    Ok(())
}

use crate::config::MarketConfig;
use crate::error::{SignerError, SignerResult};

/// Enforce CLI key allowlist the same way Python `resolve_market_key` does.
pub fn enforce_market_key_allowlist(
    market: &MarketConfig,
    allowed_key_ids: &[String],
) -> SignerResult<()> {
    let key_id = market.signer_key_id.trim();
    if key_id.is_empty() {
        return Err(SignerError::Other(format!(
            "market {} is missing signer_key_id",
            market.market_id
        )));
    }
    if allowed_key_ids.is_empty() {
        return Ok(());
    }
    if allowed_key_ids
        .iter()
        .any(|allowed| allowed.trim() == key_id)
    {
        return Ok(());
    }
    Err(SignerError::Other(format!(
        "market {} uses signer_key_id={key_id}, which is not allowed",
        market.market_id
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MarketConfig;
    use serde_json::json;
    use std::collections::HashMap;

    fn sample_market(key_id: &str) -> MarketConfig {
        MarketConfig {
            market_id: "m1".to_string(),
            enabled: true,
            base_asset: "asset1".to_string(),
            base_symbol: "AS1".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "unstable".to_string(),
            receive_address: "xch1test".to_string(),
            signer_key_id: key_id.to_string(),
            mode: "sell_only".to_string(),
            pricing: json!({}),
            cancel_move_threshold_bps: None,
            ladders: HashMap::new(),
        }
    }

    #[test]
    fn empty_allowlist_permits_any_configured_key() {
        enforce_market_key_allowlist(&sample_market("key-main-1"), &[]).expect("allowed");
    }

    #[test]
    fn configured_key_must_match_allowlist() {
        let market = sample_market("key-main-1");
        enforce_market_key_allowlist(&market, &["key-other".to_string()])
            .expect_err("not allowed");
        enforce_market_key_allowlist(&market, &["key-main-1".to_string()]).expect("allowed");
    }
}

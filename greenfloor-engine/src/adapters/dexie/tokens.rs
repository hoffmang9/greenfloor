use std::collections::HashSet;

use serde_json::Value;

use crate::error::SignerResult;

use super::client::DexieClient;

impl DexieClient {
    /// Lookup token by cat id.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn lookup_token_by_cat_id(&self, cat_id_hex: &str) -> SignerResult<Option<Value>> {
        let target = cat_id_hex.trim().to_ascii_lowercase();
        if target.is_empty() {
            return Ok(None);
        }
        for row in self.get_swap_tokens().await? {
            if row_matches_cat_target(&row, &target, false) {
                return Ok(Some(row));
            }
        }
        for row in self.get_price_tickers().await? {
            if row_matches_cat_target(&row, &target, true) {
                return Ok(Some(row));
            }
        }
        Ok(None)
    }

    /// Lookup token by symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn lookup_token_by_symbol(&self, symbol: &str) -> SignerResult<Option<Value>> {
        let target = symbol.trim();
        if target.is_empty() {
            return Ok(None);
        }
        for row in self.get_swap_tokens().await? {
            for key in ["code", "name", "id"] {
                if case_insensitive_match(
                    row.get(key).and_then(Value::as_str).unwrap_or(""),
                    target,
                ) {
                    return Ok(Some(row));
                }
            }
        }
        Ok(None)
    }
}

fn case_insensitive_match(left: &str, right: &str) -> bool {
    let a = left.trim().to_ascii_lowercase();
    let b = right.trim().to_ascii_lowercase();
    !a.is_empty() && a == b
}

fn row_matches_cat_target(row: &Value, target: &str, include_ticker_split: bool) -> bool {
    let mut candidates = HashSet::new();
    for key in [
        "assetId",
        "asset_id",
        "id",
        "tokenId",
        "token_id",
        "base_currency",
        "target_currency",
    ] {
        if let Some(value) = row.get(key).and_then(Value::as_str) {
            let trimmed = value.trim().to_ascii_lowercase();
            if !trimmed.is_empty() {
                candidates.insert(trimmed);
            }
        }
    }
    if let Some(ticker_id) = row.get("ticker_id").and_then(Value::as_str) {
        let ticker_id = ticker_id.trim().to_ascii_lowercase();
        if !ticker_id.is_empty() {
            candidates.insert(ticker_id.clone());
            if include_ticker_split {
                if let Some((base, quote)) = ticker_id.split_once('_') {
                    candidates.insert(base.to_string());
                    candidates.insert(quote.to_string());
                }
            }
        }
    }
    candidates.contains(target)
}

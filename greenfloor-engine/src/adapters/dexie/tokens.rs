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
        let (swap_tokens, price_tickers) =
            tokio::try_join!(self.get_swap_tokens(), self.get_price_tickers())?;
        for row in swap_tokens {
            if row_matches_swap_token_cat(&row, &target) {
                return Ok(Some(row));
            }
        }
        for row in price_tickers {
            if row_matches_ticker_row_cat(&row, &target) {
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

fn asset_field_candidates(row: &Value) -> HashSet<String> {
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
    candidates
}

fn add_whole_ticker_id(candidates: &mut HashSet<String>, row: &Value) {
    let Some(ticker_id) = row.get("ticker_id").and_then(Value::as_str) else {
        return;
    };
    let ticker_id = ticker_id.trim().to_ascii_lowercase();
    if !ticker_id.is_empty() {
        candidates.insert(ticker_id);
    }
}

fn add_split_ticker_pair(candidates: &mut HashSet<String>, row: &Value) {
    let Some(ticker_id) = row.get("ticker_id").and_then(Value::as_str) else {
        return;
    };
    let ticker_id = ticker_id.trim().to_ascii_lowercase();
    if ticker_id.is_empty() {
        return;
    }
    candidates.insert(ticker_id.clone());
    if let Some((base, quote)) = ticker_id.split_once('_') {
        candidates.insert(base.to_string());
        candidates.insert(quote.to_string());
    }
}

fn row_matches_swap_token_cat(row: &Value, target: &str) -> bool {
    let mut candidates = asset_field_candidates(row);
    add_whole_ticker_id(&mut candidates, row);
    candidates.contains(target)
}

fn row_matches_ticker_row_cat(row: &Value, target: &str) -> bool {
    let mut candidates = asset_field_candidates(row);
    add_split_ticker_pair(&mut candidates, row);
    candidates.contains(target)
}

#[cfg(test)]
mod tests {
    use super::{case_insensitive_match, row_matches_swap_token_cat, row_matches_ticker_row_cat};
    use serde_json::json;

    #[test]
    fn case_insensitive_match_requires_non_empty_normalized_values() {
        assert!(case_insensitive_match(" XCH ", "xch"));
        assert!(!case_insensitive_match("", "xch"));
        assert!(!case_insensitive_match("xch", "txch"));
    }

    #[test]
    fn swap_token_cat_matches_asset_id_fields() {
        let row = json!({"asset_id": "AbCd", "ticker_id": "cat_xch"});
        assert!(row_matches_swap_token_cat(&row, "abcd"));
        assert!(!row_matches_swap_token_cat(&row, "cat"));
        assert!(row_matches_swap_token_cat(&row, "cat_xch"));
    }

    #[test]
    fn ticker_row_cat_splits_ticker_pair() {
        let row = json!({"ticker_id": "cat_xch", "base_currency": "ignored"});
        assert!(row_matches_ticker_row_cat(&row, "cat"));
        assert!(row_matches_ticker_row_cat(&row, "xch"));
        assert!(row_matches_ticker_row_cat(&row, "cat_xch"));
        assert!(!row_matches_swap_token_cat(&row, "cat"));
    }
}

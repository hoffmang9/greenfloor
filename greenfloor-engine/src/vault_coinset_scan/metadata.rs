use std::collections::{HashMap, HashSet};

use crate::config::normalize_label;
use crate::hex::normalize_hex_id;

pub fn parse_csv_values(values: &[String]) -> Vec<String> {
    let mut parsed = Vec::new();
    for value in values {
        for segment in value.split(',') {
            let trimmed = segment.trim();
            if !trimmed.is_empty() {
                parsed.push(trimmed.to_string());
            }
        }
    }
    parsed
}

pub fn resolve_requested_cat_ids(
    cat_ids: &[String],
    cat_tickers: &[String],
    ticker_to_asset_ids: &HashMap<String, HashSet<String>>,
) -> (HashSet<String>, Vec<String>) {
    let mut resolved = HashSet::new();
    for raw_id in cat_ids {
        let clean = normalize_hex_id(raw_id);
        if !clean.is_empty() {
            resolved.insert(clean);
        }
    }
    let mut unresolved_tickers = Vec::new();
    for ticker in cat_tickers {
        let key = normalize_label(ticker);
        let matches = ticker_to_asset_ids.get(&key);
        if matches.is_none_or(HashSet::is_empty) {
            unresolved_tickers.push(ticker.trim().to_string());
            continue;
        }
        if let Some(matches) = matches {
            resolved.extend(matches.iter().cloned());
        }
    }
    (resolved, unresolved_tickers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_csv_values_splits_and_trims() {
        assert_eq!(
            parse_csv_values(&["a,b".to_string(), " c ".to_string()]),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn resolve_requested_cat_ids_resolves_tickers() {
        let mut ticker_map = HashMap::new();
        ticker_map.insert("wusdcb".to_string(), HashSet::from(["aa".repeat(64)]));
        let (resolved, unresolved) =
            resolve_requested_cat_ids(&[], &["wUSDC.b".to_string()], &ticker_map);
        assert!(unresolved.is_empty());
        assert_eq!(resolved.len(), 1);
        assert!(resolved.contains(&"aa".repeat(64)));
    }

    #[test]
    fn resolve_requested_cat_ids_reports_unknown_tickers() {
        let (resolved, unresolved) =
            resolve_requested_cat_ids(&[], &["NOPE".to_string()], &HashMap::new());
        assert!(resolved.is_empty());
        assert_eq!(unresolved, vec!["NOPE".to_string()]);
    }
}

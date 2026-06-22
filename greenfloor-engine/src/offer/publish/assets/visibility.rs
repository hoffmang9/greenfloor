//! Dexie offer payload asset visibility validation.

use serde_json::Value;

use super::expectations::{ExpectedPublishAssetFields, PublishAssetSide};

fn row_matches_expected(row: &Value, expected_asset: &str, expected_symbol: &str) -> bool {
    let Value::Object(obj) = row else {
        return false;
    };
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if id == expected_asset {
        return true;
    }
    if expected_symbol.is_empty() {
        return false;
    }
    for key in ["code", "name"] {
        let value = obj
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if value == expected_symbol {
            return true;
        }
    }
    false
}

fn rows_contain_expected(
    rows: &Value,
    expected_asset: &str,
    expected_symbol: &str,
) -> Option<bool> {
    match rows {
        Value::Array(values) => Some(
            values
                .iter()
                .any(|row| row_matches_expected(row, expected_asset, expected_symbol)),
        ),
        _ => None,
    }
}

fn side_expectation_error(
    rows: &Value,
    side: &PublishAssetSide,
    error_prefix: &str,
) -> Option<String> {
    let asset_id = side.asset_id.trim();
    if asset_id.is_empty() {
        return None;
    }
    let found = rows_contain_expected(
        rows,
        &asset_id.to_ascii_lowercase(),
        &side.symbol.trim().to_ascii_lowercase(),
    )?;
    if found {
        return None;
    }
    Some(format!(
        "{error_prefix}:expected_asset={asset_id}:expected_symbol={}",
        side.symbol.trim()
    ))
}

/// Return Dexie visibility asset-mismatch error text, or ``None`` when expectations are met.
///
/// Mirrors manager-side validation semantics:
/// - only validate a side when the expected asset id is non-empty
/// - only validate a side when the Dexie payload side is a list
pub(crate) fn dexie_offer_asset_expectation_error(
    offered: &Value,
    requested: &Value,
    expected: &ExpectedPublishAssetFields,
) -> Option<String> {
    side_expectation_error(
        offered,
        &expected.offered,
        "dexie_offer_offered_asset_missing",
    )
    .or_else(|| {
        side_expectation_error(
            requested,
            &expected.requested,
            "dexie_offer_requested_asset_missing",
        )
    })
}

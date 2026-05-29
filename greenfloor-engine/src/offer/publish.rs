use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedPublishAssetFields {
    pub expected_offered_asset_id: String,
    pub expected_offered_symbol: String,
    pub expected_requested_asset_id: String,
    pub expected_requested_symbol: String,
}

/// Return manager bootstrap block reason text, or ``None`` when offer creation should continue.
pub fn bootstrap_block_error(
    bootstrap_status: &str,
    bootstrap_reason: &str,
    bootstrap_ready: bool,
) -> Option<String> {
    let status = bootstrap_status.trim().to_ascii_lowercase();
    let reason = if bootstrap_reason.trim().is_empty() {
        "bootstrap_precheck_failed"
    } else {
        bootstrap_reason.trim()
    };
    if status == "failed" {
        return Some(format!("bootstrap_failed:{reason}"));
    }
    if status == "executed" && !bootstrap_ready {
        return Some(format!("bootstrap_pending:{reason}"));
    }
    if status == "skipped" && reason != "already_ready" {
        return Some(format!("bootstrap_precheck_skipped:{reason}"));
    }
    None
}

/// Resolve expected offered/requested assets for Dexie visibility checks.
pub fn expected_publish_asset_fields(
    side: &str,
    base_symbol: &str,
    quote_asset: &str,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
) -> ExpectedPublishAssetFields {
    let is_buy = side.trim().eq_ignore_ascii_case("buy");
    if is_buy {
        return ExpectedPublishAssetFields {
            expected_offered_asset_id: resolved_quote_asset_id.to_string(),
            expected_offered_symbol: quote_asset.to_string(),
            expected_requested_asset_id: resolved_base_asset_id.to_string(),
            expected_requested_symbol: base_symbol.to_string(),
        };
    }
    ExpectedPublishAssetFields {
        expected_offered_asset_id: resolved_base_asset_id.to_string(),
        expected_offered_symbol: base_symbol.to_string(),
        expected_requested_asset_id: resolved_quote_asset_id.to_string(),
        expected_requested_symbol: quote_asset.to_string(),
    }
}

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
    let code = obj
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if code == expected_symbol {
        return true;
    }
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    name == expected_symbol
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

/// Return Dexie visibility asset-mismatch error text, or ``None`` when expectations are met.
///
/// Mirrors manager-side validation semantics:
/// - only validate a side when the expected asset id is non-empty
/// - only validate a side when the Dexie payload side is a list
pub fn dexie_offer_asset_expectation_error(
    offered: &Value,
    requested: &Value,
    expected_offered_asset_id: &str,
    expected_offered_symbol: &str,
    expected_requested_asset_id: &str,
    expected_requested_symbol: &str,
) -> Option<String> {
    let expected_offered_asset = expected_offered_asset_id.trim().to_ascii_lowercase();
    let expected_offered_symbol = expected_offered_symbol.trim().to_ascii_lowercase();
    if !expected_offered_asset.is_empty() {
        if let Some(found) =
            rows_contain_expected(offered, &expected_offered_asset, &expected_offered_symbol)
        {
            if !found {
                return Some(format!(
                    "dexie_offer_offered_asset_missing:expected_asset={expected_offered_asset_id}:expected_symbol={expected_offered_symbol}"
                ));
            }
        }
    }

    let expected_requested_asset = expected_requested_asset_id.trim().to_ascii_lowercase();
    let expected_requested_symbol = expected_requested_symbol.trim().to_ascii_lowercase();
    if !expected_requested_asset.is_empty() {
        if let Some(found) = rows_contain_expected(
            requested,
            &expected_requested_asset,
            &expected_requested_symbol,
        ) {
            if !found {
                return Some(format!(
                    "dexie_offer_requested_asset_missing:expected_asset={expected_requested_asset_id}:expected_symbol={expected_requested_symbol}"
                ));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_block_error, dexie_offer_asset_expectation_error, expected_publish_asset_fields,
        ExpectedPublishAssetFields,
    };
    use serde_json::json;

    #[test]
    fn bootstrap_failed_returns_block_error() {
        assert_eq!(
            bootstrap_block_error("failed", "split_error", false),
            Some("bootstrap_failed:split_error".to_string())
        );
    }

    #[test]
    fn bootstrap_pending_when_executed_not_ready() {
        assert_eq!(
            bootstrap_block_error("executed", "split_submitted", false),
            Some("bootstrap_pending:split_submitted".to_string())
        );
    }

    #[test]
    fn bootstrap_skipped_non_ready_reason_blocks() {
        assert_eq!(
            bootstrap_block_error("skipped", "seed_missing", false),
            Some("bootstrap_precheck_skipped:seed_missing".to_string())
        );
    }

    #[test]
    fn bootstrap_allows_already_ready_skip() {
        assert_eq!(
            bootstrap_block_error("skipped", "already_ready", false),
            None
        );
    }

    #[test]
    fn bootstrap_uses_default_reason_when_missing() {
        assert_eq!(
            bootstrap_block_error("failed", "", false),
            Some("bootstrap_failed:bootstrap_precheck_failed".to_string())
        );
    }

    #[test]
    fn expected_publish_fields_for_buy_side() {
        let expected = expected_publish_asset_fields("buy", "A1", "xch", "base", "quote");
        assert_eq!(
            expected,
            ExpectedPublishAssetFields {
                expected_offered_asset_id: "quote".to_string(),
                expected_offered_symbol: "xch".to_string(),
                expected_requested_asset_id: "base".to_string(),
                expected_requested_symbol: "A1".to_string(),
            }
        );
    }

    #[test]
    fn expected_publish_fields_for_non_buy_side_defaults_to_sell() {
        let expected = expected_publish_asset_fields("anything_else", "A1", "xch", "base", "quote");
        assert_eq!(
            expected,
            ExpectedPublishAssetFields {
                expected_offered_asset_id: "base".to_string(),
                expected_offered_symbol: "A1".to_string(),
                expected_requested_asset_id: "quote".to_string(),
                expected_requested_symbol: "xch".to_string(),
            }
        );
    }

    #[test]
    fn offered_asset_matches_by_id() {
        let offered = json!([{"id": "ABCD"}]);
        let requested = json!([]);
        assert_eq!(
            dexie_offer_asset_expectation_error(&offered, &requested, "abcd", "", "", ""),
            None
        );
    }

    #[test]
    fn offered_asset_matches_by_code_or_name() {
        let offered = json!([{"code": "XCH"}, {"name": "txch"}]);
        let requested = json!([]);
        assert_eq!(
            dexie_offer_asset_expectation_error(&offered, &requested, "ff", "xch", "", ""),
            None
        );
        assert_eq!(
            dexie_offer_asset_expectation_error(&offered, &requested, "ff", "txch", "", ""),
            None
        );
    }

    #[test]
    fn returns_offered_error_when_expected_asset_missing() {
        let offered = json!([{"id": "aaaa"}]);
        let requested = json!([]);
        assert_eq!(
            dexie_offer_asset_expectation_error(&offered, &requested, "bbbb", "b", "", ""),
            Some(
                "dexie_offer_offered_asset_missing:expected_asset=bbbb:expected_symbol=b"
                    .to_string()
            )
        );
    }

    #[test]
    fn returns_requested_error_when_expected_asset_missing() {
        let offered = json!([]);
        let requested = json!([{"id": "xch"}]);
        assert_eq!(
            dexie_offer_asset_expectation_error(&offered, &requested, "", "", "cat", "cat"),
            Some(
                "dexie_offer_requested_asset_missing:expected_asset=cat:expected_symbol=cat"
                    .to_string()
            )
        );
    }

    #[test]
    fn skips_validation_when_payload_side_is_not_a_list() {
        let offered = json!({"id": "xch"});
        let requested = json!({"id": "cat"});
        assert_eq!(
            dexie_offer_asset_expectation_error(&offered, &requested, "xch", "xch", "cat", "cat"),
            None
        );
    }
}

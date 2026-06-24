use chia_protocol::Coin;
use chia_protocol::CoinSpend;
use chia_sdk_coinset::{CoinRecord, GetCoinRecordsResponse, PushTxResponse};
use serde_json::Value;

use crate::error::{SignerError, SignerResult};
use crate::hex::hex_to_bytes32;
use crate::hex::normalize_hex_id;

pub(crate) trait CoinsetRpcResponse {
    fn is_success(&self) -> bool;
    fn error_message(&self) -> Option<&str>;
}

impl CoinsetRpcResponse for GetCoinRecordsResponse {
    fn is_success(&self) -> bool {
        self.success
    }

    fn error_message(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl CoinsetRpcResponse for PushTxResponse {
    fn is_success(&self) -> bool {
        self.success
    }

    fn error_message(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

pub(crate) fn ensure_coinset_typed_rpc_success<T: CoinsetRpcResponse>(
    response: &T,
    failure_default: &str,
) -> SignerResult<()> {
    if response.is_success() {
        return Ok(());
    }
    Err(SignerError::Coinset(response.error_message().map_or_else(
        || failure_default.to_string(),
        str::to_string,
    )))
}

pub(crate) fn unspent_coin_records(records: Vec<CoinRecord>) -> impl Iterator<Item = CoinRecord> {
    records.into_iter().filter(|record| !record.spent)
}

fn coinset_rpc_failure_detail(payload: &Value) -> String {
    for key in ["error", "error_message", "message"] {
        if let Some(message) = payload.get(key).and_then(Value::as_str) {
            let trimmed = message.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    "coinset rpc returned success=false".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoinsetRecordsPagination {
    pub truncated: bool,
    pub next_cursor: Option<String>,
}

pub(crate) fn pagination_from_response(
    response: &GetCoinRecordsResponse,
) -> CoinsetRecordsPagination {
    CoinsetRecordsPagination {
        truncated: response.truncated.unwrap_or(false),
        next_cursor: response.next_cursor.clone(),
    }
}

pub(crate) fn pagination_from_payload(payload: &Value) -> CoinsetRecordsPagination {
    CoinsetRecordsPagination {
        truncated: payload
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_cursor: payload
            .get("next_cursor")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// Reject truncated Coinset pages that omit the resume cursor.
///
/// # Errors
///
/// Returns an error when `truncated` is true and `next_cursor` is missing.
pub(crate) fn ensure_complete_page(pagination: &CoinsetRecordsPagination) -> SignerResult<()> {
    if pagination.truncated && pagination.next_cursor.is_none() {
        return Err(SignerError::Coinset(
            "coinset response truncated without next_cursor".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn coin_records_from_response(
    response: GetCoinRecordsResponse,
) -> SignerResult<Vec<CoinRecord>> {
    let (records, _) = coin_records_page_from_response(response)?;
    Ok(records)
}

pub(crate) fn coin_records_page_from_response(
    response: GetCoinRecordsResponse,
) -> SignerResult<(Vec<CoinRecord>, CoinsetRecordsPagination)> {
    ensure_coinset_typed_rpc_success(&response, "coinset request failed")?;
    let pagination = pagination_from_response(&response);
    ensure_complete_page(&pagination)?;
    Ok((response.coin_records.unwrap_or_default(), pagination))
}

/// Ensure coinset rpc success.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn ensure_coinset_rpc_success(payload: &Value) -> SignerResult<()> {
    if payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(());
    }
    Err(SignerError::Coinset(coinset_rpc_failure_detail(payload)))
}

/// Coin records from payload.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn coin_records_from_payload(payload: &Value) -> SignerResult<Vec<Value>> {
    ensure_coinset_rpc_success(payload)?;
    Ok(payload
        .get("coin_records")
        .and_then(Value::as_array)
        .map(|records| {
            records
                .iter()
                .filter(|record| record.is_object())
                .cloned()
                .collect()
        })
        .unwrap_or_default())
}

/// Record from payload.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn record_from_payload<'a>(payload: &'a Value, key: &str) -> SignerResult<Option<&'a Value>> {
    ensure_coinset_rpc_success(payload)?;
    Ok(payload.get(key).filter(|value| value.is_object()))
}

fn normalized_hex_field(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(normalize_hex_id)
        .unwrap_or_default()
}

pub fn coin_id_from_record(record: &Value) -> String {
    let Some(coin) = record.get("coin").and_then(Value::as_object) else {
        return String::new();
    };
    for key in ["name", "coin_id", "coin_name"] {
        let normalized = normalized_hex_field(coin.get(key));
        if !normalized.is_empty() {
            return normalized;
        }
    }
    let normalized = normalized_hex_field(record.get("name"));
    if !normalized.is_empty() {
        return normalized;
    }

    let parent_hex = normalized_hex_field(coin.get("parent_coin_info"));
    let puzzle_hex = normalized_hex_field(coin.get("puzzle_hash"));
    let amount = coin.get("amount").and_then(Value::as_u64);
    if parent_hex.is_empty() || puzzle_hex.is_empty() {
        return String::new();
    }
    let Some(amount) = amount else {
        return String::new();
    };

    let Ok(parent) = hex_to_bytes32(&parent_hex) else {
        return String::new();
    };
    let Ok(puzzle_hash) = hex_to_bytes32(&puzzle_hex) else {
        return String::new();
    };
    hex::encode(Coin::new(parent, puzzle_hash, amount).coin_id())
}

#[must_use]
pub fn to_coinset_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

#[must_use]
pub fn u64_from_value(value: Option<&Value>, default: u64) -> u64 {
    value
        .and_then(|raw| {
            raw.as_u64()
                .or_else(|| raw.as_i64().and_then(|v| u64::try_from(v).ok()))
        })
        .unwrap_or(default)
}

pub fn coin_from_record(record: &Value) -> Option<Coin> {
    let coin = record.get("coin")?;
    let parent_hex = normalize_hex_id(coin.get("parent_coin_info")?.as_str()?);
    let puzzle_hex = normalize_hex_id(coin.get("puzzle_hash")?.as_str()?);
    if parent_hex.is_empty() || puzzle_hex.is_empty() {
        return None;
    }
    let parent = hex_to_bytes32(&parent_hex).ok()?;
    let puzzle_hash = hex_to_bytes32(&puzzle_hex).ok()?;
    let amount = coin.get("amount").and_then(Value::as_u64)?;
    Some(Coin::new(parent, puzzle_hash, amount))
}

#[must_use]
pub fn coin_spend_from_solution_payload(parent_coin: Coin, solution: &Value) -> Option<CoinSpend> {
    let puzzle_reveal_hex = solution.get("puzzle_reveal")?.as_str()?.trim();
    let solution_hex = solution.get("solution")?.as_str()?.trim();
    if puzzle_reveal_hex.is_empty() || solution_hex.is_empty() {
        return None;
    }
    let puzzle_reveal = decode_hex_bytes(puzzle_reveal_hex).ok()?;
    let solution_bytes = decode_hex_bytes(solution_hex).ok()?;
    Some(CoinSpend::new(
        parent_coin,
        puzzle_reveal.into(),
        solution_bytes.into(),
    ))
}

fn decode_hex_bytes(raw: &str) -> Result<Vec<u8>, hex::FromHexError> {
    hex::decode(raw.trim_start_matches("0x"))
}

pub fn chunk_values<T: Clone>(values: &[T], chunk_size: usize) -> Vec<Vec<T>> {
    if chunk_size == 0 {
        return if values.is_empty() {
            Vec::new()
        } else {
            vec![values.to_vec()]
        };
    }
    values.chunks(chunk_size).map(<[T]>::to_vec).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coin_records_from_payload_filters_non_objects() {
        let payload = json!({
            "success": true,
            "coin_records": [{"coin": {"amount": 1}}, "bad"]
        });
        let records = coin_records_from_payload(&payload).expect("coin records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["coin"]["amount"], 1);
    }

    #[test]
    fn coin_records_from_payload_errors_on_rpc_failure() {
        let payload = json!({"success": false, "error": "invalid puzzle hash"});
        let err = coin_records_from_payload(&payload).expect_err("rpc failure");
        assert_eq!(err.to_string(), "coinset error: invalid puzzle hash");
    }

    #[test]
    fn record_from_payload_errors_on_rpc_failure() {
        let payload = json!({"success": false, "coin_record": {"coin": {"amount": 1}}});
        let err = record_from_payload(&payload, "coin_record").expect_err("rpc failure");
        assert!(err.to_string().contains("success=false"));
    }

    #[test]
    fn record_from_payload_returns_none_when_record_missing_on_success() {
        let payload = json!({"success": true});
        assert!(record_from_payload(&payload, "coin_record")
            .expect("success payload")
            .is_none());
    }

    #[test]
    fn chunk_values_respects_batch_size() {
        let values = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(
            chunk_values(&values, 2),
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string()]
            ]
        );
    }

    #[test]
    fn to_coinset_hex_prefixes_0x() {
        assert_eq!(to_coinset_hex(&[0xab]), "0xab");
    }

    #[test]
    fn coin_id_from_record_prefers_explicit_name_field() {
        let name = "a".repeat(64);
        let record = json!({
            "coin": {
                "parent_coin_info": format!("0x{}", "b".repeat(64)),
                "puzzle_hash": format!("0x{}", "c".repeat(64)),
                "amount": 1,
                "name": format!("0x{name}"),
            }
        });
        assert_eq!(coin_id_from_record(&record), name);
    }

    #[test]
    fn coin_id_from_record_computes_from_parent_puzzle_and_amount() {
        use chia_protocol::{Bytes32, Coin};

        let parent = Bytes32::new([0x11; 32]);
        let puzzle_hash = Bytes32::new([0x22; 32]);
        let amount = 42_u64;
        let expected = hex::encode(Coin::new(parent, puzzle_hash, amount).coin_id());
        let record = json!({
            "coin": {
                "parent_coin_info": format!("0x{}", hex::encode(parent)),
                "puzzle_hash": format!("0x{}", hex::encode(puzzle_hash)),
                "amount": amount,
            }
        });
        assert_eq!(coin_id_from_record(&record), expected);
    }

    #[test]
    fn coin_id_from_record_returns_empty_when_coin_missing() {
        assert!(coin_id_from_record(&json!({})).is_empty());
    }

    #[test]
    fn u64_from_value_prefers_u64_and_parses_i64() {
        assert_eq!(u64_from_value(Some(&json!(42_u64)), 0), 42);
        assert_eq!(u64_from_value(Some(&json!(7_i64)), 0), 7);
        assert_eq!(u64_from_value(Some(&json!("bad")), 99), 99);
        assert_eq!(u64_from_value(None, 5), 5);
    }

    #[test]
    fn coin_from_record_builds_coin_from_nested_payload() {
        use chia_protocol::{Bytes32, Coin};

        let parent = Bytes32::new([0x11; 32]);
        let puzzle_hash = Bytes32::new([0x22; 32]);
        let record = json!({
            "coin": {
                "parent_coin_info": format!("0x{}", hex::encode(parent)),
                "puzzle_hash": format!("0x{}", hex::encode(puzzle_hash)),
                "amount": 99,
            }
        });
        let coin = coin_from_record(&record).expect("coin");
        assert_eq!(coin, Coin::new(parent, puzzle_hash, 99));
        assert!(coin_from_record(&json!({"coin": {"amount": 1}})).is_none());
    }

    #[test]
    fn coin_spend_from_solution_payload_decodes_hex_fields() {
        use chia_protocol::{Bytes32, Coin};

        let parent = Coin::new(Bytes32::new([0x01; 32]), Bytes32::new([0x02; 32]), 1);
        let puzzle = "0102";
        let solution = "0304";
        let spend = coin_spend_from_solution_payload(
            parent,
            &json!({
                "puzzle_reveal": format!("0x{puzzle}"),
                "solution": solution,
            }),
        )
        .expect("spend");
        assert_eq!(hex::encode(spend.puzzle_reveal.as_ref()), puzzle);
        assert_eq!(hex::encode(spend.solution.as_ref()), solution);
    }

    #[test]
    fn chunk_values_zero_batch_returns_single_chunk() {
        let values = vec![1, 2, 3];
        assert_eq!(chunk_values(&values, 0), vec![vec![1, 2, 3]]);
        assert!(chunk_values::<i32>(&[], 2).is_empty());
    }

    #[test]
    fn pagination_from_payload_reads_truncated_and_cursor() {
        let payload = json!({
            "success": true,
            "truncated": true,
            "next_cursor": "abc",
            "coin_records": []
        });
        let pagination = pagination_from_payload(&payload);
        assert!(pagination.truncated);
        assert_eq!(pagination.next_cursor.as_deref(), Some("abc"));
    }

    #[test]
    fn ensure_complete_page_errors_when_truncated_without_cursor() {
        let pagination = CoinsetRecordsPagination {
            truncated: true,
            next_cursor: None,
        };
        let err = ensure_complete_page(&pagination).expect_err("missing cursor");
        assert!(err.to_string().contains("truncated without next_cursor"));
    }
}

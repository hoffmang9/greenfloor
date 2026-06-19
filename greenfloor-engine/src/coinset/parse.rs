use chia_protocol::Coin;
use chia_protocol::CoinSpend;
use serde_json::Value;

use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::vault::members::hex_to_bytes32;

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

pub fn to_coinset_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

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
}

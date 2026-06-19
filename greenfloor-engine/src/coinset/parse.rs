use serde_json::Value;

use crate::error::{SignerError, SignerResult};

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
}

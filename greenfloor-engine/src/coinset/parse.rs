use serde_json::Value;

pub fn coin_records_from_payload(payload: &Value) -> Vec<Value> {
    if !payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Vec::new();
    }
    payload
        .get("coin_records")
        .and_then(Value::as_array)
        .map(|records| {
            records
                .iter()
                .filter(|record| record.is_object())
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub fn record_from_payload<'a>(payload: &'a Value, key: &str) -> Option<&'a Value> {
    if !payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    payload.get(key).filter(|value| value.is_object())
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
        let records = coin_records_from_payload(&payload);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["coin"]["amount"], 1);
    }

    #[test]
    fn coin_records_from_payload_returns_empty_on_failure() {
        let payload = json!({"success": false});
        assert!(coin_records_from_payload(&payload).is_empty());
    }

    #[test]
    fn record_from_payload_returns_none_on_failure() {
        let payload = json!({"success": false, "coin_record": {"coin": {"amount": 1}}});
        assert!(record_from_payload(&payload, "coin_record").is_none());
    }
}

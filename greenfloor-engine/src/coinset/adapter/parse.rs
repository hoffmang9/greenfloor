use serde_json::Value;

pub(crate) fn coin_records_from_payload(payload: &Value) -> Vec<Value> {
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

pub(crate) fn record_from_payload<'a>(payload: &'a Value, key: &str) -> Option<&'a Value> {
    if !payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    payload.get(key).filter(|value| value.is_object())
}

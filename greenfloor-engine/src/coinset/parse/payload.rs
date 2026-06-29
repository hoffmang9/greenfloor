use serde_json::Value;

use crate::coinset::rpc_result::ensure_coinset_rpc_success;
use crate::error::SignerResult;

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

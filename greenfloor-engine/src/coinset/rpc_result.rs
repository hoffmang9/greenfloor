use serde_json::Value;

use crate::error::{SignerError, SignerResult};

pub(crate) fn ensure_coinset_success(
    success: bool,
    error: Option<&str>,
    failure_default: &str,
) -> SignerResult<()> {
    if success {
        return Ok(());
    }
    Err(SignerError::Coinset(error.map_or_else(
        || failure_default.to_string(),
        str::to_string,
    )))
}

fn coinset_error_from_payload(payload: &Value) -> Option<&str> {
    for key in ["error", "error_message", "message"] {
        if let Some(message) = payload.get(key).and_then(Value::as_str) {
            let trimmed = message.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

/// Ensure coinset rpc success.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn ensure_coinset_rpc_success(payload: &Value) -> SignerResult<()> {
    ensure_coinset_success(
        payload
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        coinset_error_from_payload(payload),
        "coinset rpc returned success=false",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ensure_coinset_rpc_success_reads_error_field() {
        let payload = json!({"success": false, "error": "invalid puzzle hash"});
        let err = ensure_coinset_rpc_success(&payload).expect_err("rpc failure");
        assert_eq!(err.to_string(), "coinset error: invalid puzzle hash");
    }

    #[test]
    fn ensure_coinset_rpc_success_falls_back_to_default_message() {
        let payload = json!({"success": false});
        let err = ensure_coinset_rpc_success(&payload).expect_err("rpc failure");
        assert!(err.to_string().contains("success=false"));
    }

    #[test]
    fn ensure_coinset_success_prefers_error_over_default() {
        let err =
            ensure_coinset_success(false, Some("typed failure"), "default").expect_err("failure");
        assert_eq!(err.to_string(), "coinset error: typed failure");
    }
}

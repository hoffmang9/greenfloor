//! Shared CLI helpers for manager and daemon entrypoints.

use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{SignerError, SignerResult};

const RETRYABLE_COINSET_TRANSPORT_MARKERS: &[&str] = &[
    "operation timed out",
    "connection refused",
    "connection reset",
    "remote end closed connection",
    "error sending request",
    "temporary failure",
    "temporarily unavailable",
    "broken pipe",
    "http status server error (502",
    "http status server error (503",
    "http status server error (504",
    "http status client error (429",
    "too many requests",
    "bad gateway",
    "service unavailable",
    "error decoding response body",
    "ssl",
    "handshake",
    "cloudflare",
];

#[must_use]
pub fn script_coinset_transport_retryable(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    RETRYABLE_COINSET_TRANSPORT_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

#[must_use]
pub fn script_engine_error_retryable(err: &SignerError) -> bool {
    match err {
        SignerError::Coinset(message) => script_coinset_transport_retryable(message),
        _ => false,
    }
}

pub fn emit_engine_cli_error(err: &SignerError, json_mode: bool) {
    if json_mode {
        let payload = json!({
            "success": false,
            "error": err.to_string(),
            "retryable": script_engine_error_retryable(err),
        });
        eprintln!(
            "{}",
            serde_json::to_string(&payload).unwrap_or_else(|_| {
                format!(
                    r#"{{"success":false,"error":{},"retryable":false}}"#,
                    serde_json::to_string(&err.to_string())
                        .unwrap_or_else(|_| "\"unknown\"".to_string())
                )
            })
        );
    } else {
        eprintln!("error: {err}");
    }
}

#[must_use]
pub fn optional_str(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub fn optional_trimmed(value: &str) -> Option<String> {
    optional_str(value).map(str::to_string)
}

/// Format json.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn format_json(value: &impl Serialize, compact: bool) -> Result<String, String> {
    if compact {
        serde_json::to_string(value).map_err(|err| format!("failed to encode json output: {err}"))
    } else {
        serde_json::to_string_pretty(value)
            .map_err(|err| format!("failed to encode json output: {err}"))
    }
}

/// Format json value.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn format_json_value(value: &Value, compact: bool) -> Result<String, String> {
    if compact {
        serde_json::to_string(value).map_err(|err| format!("failed to encode json output: {err}"))
    } else {
        serde_json::to_string_pretty(value)
            .map_err(|err| format!("failed to encode json output: {err}"))
    }
}

/// Print json.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn print_json(value: &impl Serialize, compact: bool) -> SignerResult<()> {
    println!(
        "{}",
        format_json(value, compact).map_err(SignerError::Other)?
    );
    Ok(())
}

/// Print json value.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn print_json_value(value: &Value, compact: bool) -> SignerResult<()> {
    println!(
        "{}",
        format_json_value(value, compact).map_err(SignerError::Other)?
    );
    Ok(())
}

/// Print json pretty.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn print_json_pretty(value: &impl Serialize) -> SignerResult<()> {
    print_json(value, false)
}

#[cfg(test)]
mod tests {
    use super::{
        emit_engine_cli_error, format_json, format_json_value, optional_str, optional_trimmed,
        script_coinset_transport_retryable, script_engine_error_retryable,
    };
    use crate::error::SignerError;
    use serde_json::json;

    #[test]
    fn script_coinset_transport_retryable_matches_decode_and_refused() {
        assert!(script_coinset_transport_retryable(
            "error decoding response body"
        ));
        assert!(script_coinset_transport_retryable(
            "error sending request for url (http://127.0.0.1:1/): connection refused"
        ));
        assert!(!script_coinset_transport_retryable("invalid puzzle hash"));
    }

    #[test]
    fn script_engine_error_retryable_classifies_coinset_and_parse_errors() {
        assert!(script_engine_error_retryable(&SignerError::Coinset(
            "error decoding response body".to_string()
        )));
        assert!(!script_engine_error_retryable(&SignerError::Other(
            "parse body json: expected value at line 1 column 1".to_string()
        )));
    }

    #[test]
    fn optional_str_trims_and_rejects_blank() {
        assert_eq!(optional_str("  value  "), Some("value"));
        assert_eq!(optional_str(""), None);
        assert_eq!(optional_str("   "), None);
        assert_eq!(optional_trimmed("  x  "), Some("x".to_string()));
        assert_eq!(optional_trimmed(""), None);
    }

    #[test]
    fn format_json_respects_compact_flag() {
        let payload = json!({"ok": true, "n": 1});
        assert!(format_json(&payload, false).unwrap().contains('\n'));
        assert_eq!(format_json(&payload, true).unwrap(), r#"{"n":1,"ok":true}"#);
        assert_eq!(
            format_json_value(&payload, true).unwrap(),
            r#"{"n":1,"ok":true}"#
        );
    }

    #[test]
    fn emit_engine_cli_error_json_mode_marks_retryable_coinset_errors() {
        let err = SignerError::Coinset("connection reset by peer".to_string());
        emit_engine_cli_error(&err, true);
        emit_engine_cli_error(&SignerError::Other("bad input".to_string()), true);
        emit_engine_cli_error(&SignerError::Other("bad input".to_string()), false);
    }
}

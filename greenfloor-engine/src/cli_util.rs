//! Shared CLI helpers for manager and daemon entrypoints.

use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{SignerError, SignerResult};

pub fn emit_engine_cli_error(err: &SignerError, json_mode: bool) {
    if json_mode {
        let payload = json!({
            "success": false,
            "error": err.to_string(),
            "retryable": err.is_retryable_upstream(),
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
    use super::{format_json, format_json_value, optional_str, optional_trimmed};
    use serde_json::json;

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
}

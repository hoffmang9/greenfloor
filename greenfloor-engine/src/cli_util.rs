//! Shared CLI helpers for manager and daemon entrypoints.

use serde::Serialize;
use serde_json::Value;

use crate::error::{SignerError, SignerResult};

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

pub fn format_json(value: &impl Serialize, compact: bool) -> Result<String, String> {
    if compact {
        serde_json::to_string(value).map_err(|err| format!("failed to encode json output: {err}"))
    } else {
        serde_json::to_string_pretty(value)
            .map_err(|err| format!("failed to encode json output: {err}"))
    }
}

pub fn format_json_value(value: &Value, compact: bool) -> Result<String, String> {
    if compact {
        serde_json::to_string(value).map_err(|err| format!("failed to encode json output: {err}"))
    } else {
        serde_json::to_string_pretty(value)
            .map_err(|err| format!("failed to encode json output: {err}"))
    }
}

pub fn print_json(value: &impl Serialize, compact: bool) -> SignerResult<()> {
    println!(
        "{}",
        format_json(value, compact).map_err(SignerError::Other)?
    );
    Ok(())
}

pub fn print_json_value(value: &Value, compact: bool) -> SignerResult<()> {
    println!(
        "{}",
        format_json_value(value, compact).map_err(SignerError::Other)?
    );
    Ok(())
}

pub fn print_json_pretty(value: &impl Serialize) -> SignerResult<()> {
    print_json(value, false)
}

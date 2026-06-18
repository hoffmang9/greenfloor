//! Shared CLI helpers for manager and daemon entrypoints.

use serde::Serialize;

use crate::error::{SignerError, SignerResult};

pub fn optional_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn print_json_pretty(value: &impl Serialize) -> SignerResult<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|err| {
            SignerError::Other(format!("failed to encode json output: {err}"))
        })?
    );
    Ok(())
}

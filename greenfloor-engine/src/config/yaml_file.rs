//! Read/write YAML config files as [`serde_json::Value`].

use std::path::Path;

use serde_json::Value;

use crate::error::{SignerError, SignerResult};

/// Read a YAML file, using `label` in error messages (for example `"config"`).
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn read_yaml_file_labeled(path: &Path, label: &str) -> SignerResult<Value> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!("failed to read {label} {}: {err}", path.display()))
    })?;
    serde_yaml::from_str(&raw).map_err(|err| {
        SignerError::Other(format!("failed to parse {label} {}: {err}", path.display()))
    })
}

/// Write a JSON value tree to a YAML file.
///
/// # Errors
///
/// Returns an error if encoding or writing fails.
pub fn write_yaml_file(path: &Path, value: &Value) -> SignerResult<()> {
    let text = serde_yaml::to_string(value)
        .map_err(|err| SignerError::Other(format!("failed to encode yaml: {err}")))?;
    std::fs::write(path, text)
        .map_err(|err| SignerError::Other(format!("failed to write {}: {err}", path.display())))
}

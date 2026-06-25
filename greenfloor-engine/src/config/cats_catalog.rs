//! Cats catalog YAML load/write (operator metadata).

use std::path::Path;

use serde_json::{json, Value as JsonValue};

use crate::config::yaml_file::{read_yaml_file_labeled, write_yaml_file};
use crate::error::SignerResult;

/// Load cats catalog rows from `cats.yaml`.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load_cats_catalog(path: &Path) -> SignerResult<Vec<JsonValue>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let parsed = read_yaml_file_labeled(path, "cats config")?;
    Ok(parsed
        .get("cats")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default())
}

/// Write cats catalog rows to `cats.yaml`.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_cats_catalog(path: &Path, catalog: &[JsonValue]) -> SignerResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            crate::error::SignerError::Other(format!(
                "failed to create {}: {err}",
                parent.display()
            ))
        })?;
    }
    write_yaml_file(path, &json!({"cats": catalog}))
}

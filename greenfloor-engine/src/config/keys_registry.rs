//! Signer key registry parsing shared by validation and program config build.

use std::collections::HashMap;

use serde_json::Value;

use super::yaml_fields::{
    config_err, optional_trimmed_string, parse_i64_field, req_str, req_value,
};
use crate::error::SignerResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerKeyEntry {
    pub key_id: String,
    pub fingerprint: u64,
    pub network: Option<String>,
    pub keyring_yaml_path: Option<String>,
}

pub fn parse_signer_key_registry(raw: &Value) -> SignerResult<HashMap<String, SignerKeyEntry>> {
    let keys_root = raw.get("keys").and_then(Value::as_object);
    let registry_rows = keys_root
        .and_then(|keys| keys.get("registry"))
        .unwrap_or(&Value::Null);

    let rows: Vec<&Value> = if registry_rows.is_null() {
        Vec::new()
    } else {
        registry_rows
            .as_array()
            .ok_or_else(|| config_err("keys.registry must be a list"))?
            .iter()
            .collect()
    };

    let mut key_registry = HashMap::default();
    for row in rows {
        let row_map = row
            .as_object()
            .ok_or_else(|| config_err("keys.registry entries must be mappings"))?;
        let key_id = req_str(row_map, "key_id")?.trim().to_string();
        if key_id.is_empty() {
            return Err(config_err("keys.registry entry key_id must be non-empty"));
        }
        let fingerprint_raw = req_value(row_map, "fingerprint")?;
        let fingerprint_field = format!("fingerprint for key_id={key_id}");
        let fingerprint = parse_i64_field(fingerprint_raw, &fingerprint_field)
            .map_err(|_| config_err(format!("invalid fingerprint for key_id={key_id}")))?;
        if fingerprint <= 0 {
            return Err(config_err(format!(
                "fingerprint for key_id={key_id} must be positive"
            )));
        }
        if key_registry.contains_key(&key_id) {
            return Err(config_err(format!(
                "duplicate key_id in keys.registry: {key_id}"
            )));
        }
        key_registry.insert(
            key_id.clone(),
            SignerKeyEntry {
                key_id,
                fingerprint: crate::config::non_negative_i64_to_u64(
                    fingerprint,
                    &fingerprint_field,
                )?,
                network: optional_trimmed_string(row_map.get("network")),
                keyring_yaml_path: optional_trimmed_string(row_map.get("keyring_yaml_path")),
            },
        );
    }
    Ok(key_registry)
}

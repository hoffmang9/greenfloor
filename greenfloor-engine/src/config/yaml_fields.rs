//! Shared YAML field accessors for config parsing and validation.

use serde_json::Value;

use crate::error::{SignerError, SignerResult};

pub fn config_err(message: impl Into<String>) -> SignerError {
    SignerError::Other(message.into())
}

pub fn req_mapping<'a>(
    value: &'a Value,
    key: &str,
) -> SignerResult<&'a serde_json::Map<String, Value>> {
    match value.get(key) {
        Some(Value::Object(map)) => Ok(map),
        Some(_) => Err(config_err(format!("{key} must be a mapping"))),
        None => Err(config_err(format!("Missing required field: {key}"))),
    }
}

pub fn req_mapping_from_map<'a>(
    map: &'a serde_json::Map<String, Value>,
    key: &str,
) -> SignerResult<&'a serde_json::Map<String, Value>> {
    match map.get(key) {
        Some(Value::Object(nested)) => Ok(nested),
        Some(_) => Err(config_err(format!("{key} must be a mapping"))),
        None => Err(config_err(format!("Missing required field: {key}"))),
    }
}

pub fn req_value<'a>(
    map: &'a serde_json::Map<String, Value>,
    key: &str,
) -> SignerResult<&'a Value> {
    map.get(key)
        .ok_or_else(|| config_err(format!("Missing required field: {key}")))
}

pub fn req_str(map: &serde_json::Map<String, Value>, key: &str) -> SignerResult<String> {
    Ok(req_value(map, key)?
        .as_str()
        .ok_or_else(|| config_err(format!("Missing required field: {key}")))?
        .to_string())
}

pub fn optional_bool(map: &serde_json::Map<String, Value>, key: &str, default: bool) -> bool {
    map.get(key).and_then(Value::as_bool).unwrap_or(default)
}

pub fn parse_i64_field(raw: &Value, context: &str) -> SignerResult<i64> {
    if let Some(value) = raw.as_i64() {
        return Ok(value);
    }
    if let Some(value) = raw.as_u64() {
        return i64::try_from(value).map_err(|_| config_err(format!("{context} must fit in i64")));
    }
    if let Some(text) = raw.as_str() {
        if let Ok(value) = text.parse::<i64>() {
            return Ok(value);
        }
    }
    Err(config_err(format!("{context} must be an integer")))
}

pub fn parse_u64_field(raw: &Value, context: &str) -> SignerResult<u64> {
    let value = parse_i64_field(raw, context)?;
    if value < 0 {
        return Err(config_err(format!("{context} must be >= 0")));
    }
    u64::try_from(value).map_err(|_| config_err(format!("{context} must fit in u64")))
}

pub fn parse_f64_field(raw: &Value, context: &str) -> SignerResult<f64> {
    if let Some(value) = raw.as_f64() {
        return Ok(value);
    }
    if let Some(value) = raw.as_i64() {
        return Ok(crate::offer::pricing::i64_to_f64(value));
    }
    if let Some(value) = raw.as_u64() {
        return Ok(crate::offer::pricing::u64_to_f64(value));
    }
    if let Some(text) = raw.as_str() {
        if let Ok(value) = text.parse::<f64>() {
            return Ok(value);
        }
    }
    Err(config_err(format!("{context} must be numeric")))
}

pub fn optional_str(map: &serde_json::Map<String, Value>, key: &str, default: &str) -> String {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(|| default.to_string(), str::to_string)
}

pub fn optional_str_section(
    section: Option<&serde_json::Map<String, Value>>,
    key: &str,
    default: &str,
) -> String {
    match section {
        Some(map) => optional_str(map, key, default),
        None => default.to_string(),
    }
}

pub fn optional_trimmed_str_section(
    section: Option<&serde_json::Map<String, Value>>,
    key: &str,
) -> String {
    section
        .and_then(|map| map.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

pub fn optional_i64(
    map: &serde_json::Map<String, Value>,
    key: &str,
    default: i64,
) -> SignerResult<i64> {
    match map.get(key) {
        None => Ok(default),
        Some(raw) => parse_i64_field(raw, key),
    }
}

pub fn optional_f64(
    map: &serde_json::Map<String, Value>,
    key: &str,
    default: f64,
) -> SignerResult<f64> {
    match map.get(key) {
        None => Ok(default),
        Some(raw) => parse_f64_field(raw, key),
    }
}

pub fn optional_bool_value(raw: Option<&Value>, default: bool) -> bool {
    raw.and_then(Value::as_bool).unwrap_or(default)
}

pub fn optional_trimmed_string(raw: Option<&Value>) -> Option<String> {
    raw.and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

use serde_json::Value;

use super::super::yaml_fields::parse_i64_field;
use crate::error::SignerResult;

pub(super) fn venues_subsection<'a>(
    raw: &'a Value,
    name: &str,
) -> Option<&'a serde_json::Map<String, Value>> {
    raw.get("venues")
        .and_then(Value::as_object)
        .and_then(|venues| venues.get(name))
        .and_then(Value::as_object)
}

pub(super) fn normalize_api_base(raw: Option<&Value>, default: &str) -> String {
    raw.and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
        .trim_end_matches('/')
        .to_string()
}

pub(super) fn coin_ops_i64_field(
    section: Option<&serde_json::Map<String, Value>>,
    key: &str,
    default: i64,
) -> SignerResult<i64> {
    parse_i64_field(
        section
            .and_then(|map| map.get(key))
            .unwrap_or(&Value::Number(default.into())),
        &format!("coin_ops.{key}"),
    )
}

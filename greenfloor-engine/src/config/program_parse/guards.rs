use serde_json::Value;

use super::super::yaml_fields::{config_err, req_mapping, req_value};
use crate::error::SignerResult;

pub(super) fn reject_cloud_wallet(raw: &Value) -> SignerResult<()> {
    match raw.get("cloud_wallet") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Object(map)) if map.is_empty() => Ok(()),
        Some(_) => Err(config_err(
            "cloud_wallet config is removed; use signer: and vault: blocks instead \
             (see config/program.yaml)",
        )),
    }
}

pub(super) fn require_pushover_provider(raw: &Value) -> SignerResult<()> {
    let notifications = req_mapping(raw, "notifications")?;
    req_value(notifications, "low_inventory_alerts")?;
    let providers = req_value(notifications, "providers")?
        .as_array()
        .ok_or_else(|| config_err("notifications.providers must be a list"))?;
    if providers.iter().any(|provider| {
        provider
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| value.trim() == "pushover")
    }) {
        return Ok(());
    }
    Err(config_err(
        "Missing notifications.providers entry with type=pushover",
    ))
}

pub(super) fn parse_dev_python_min_version(
    dev: &serde_json::Map<String, Value>,
) -> SignerResult<String> {
    use super::super::yaml_fields::req_mapping_from_map;

    let python = req_mapping_from_map(dev, "python")?;
    match python.get("min_version") {
        None => Ok("3.11".to_string()),
        Some(value) => {
            let text = value
                .as_str()
                .ok_or_else(|| config_err("dev.python.min_version must be a string"))?;
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Err(config_err(
                    "dev.python.min_version must be non-empty when set",
                ));
            }
            Ok(trimmed.to_string())
        }
    }
}

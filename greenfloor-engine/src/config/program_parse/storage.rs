use serde_json::Value;

use super::super::yaml_fields::parse_u64_field;
use crate::error::SignerResult;
use crate::storage::DEFAULT_AUDIT_RETENTION_DAYS;

pub(super) struct StorageFields {
    pub storage_audit_retention_days: u64,
}

pub(super) fn parse_storage_config(
    storage: Option<&serde_json::Map<String, Value>>,
) -> SignerResult<StorageFields> {
    let section = storage.cloned().unwrap_or_default();
    Ok(StorageFields {
        storage_audit_retention_days: parse_u64_field(
            section
                .get("audit_retention_days")
                .unwrap_or(&Value::Number(DEFAULT_AUDIT_RETENTION_DAYS.into())),
            "storage.audit_retention_days",
        )?
        .max(1),
    })
}

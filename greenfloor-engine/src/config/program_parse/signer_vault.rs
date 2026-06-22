use serde_json::Value;

use super::super::yaml_fields::{optional_str_section, optional_trimmed_str_section};

pub(super) struct SignerVaultFields {
    pub signer_kms_key_id: String,
    pub signer_kms_region: String,
    pub vault_launcher_id: String,
}

pub(super) fn parse_signer_vault_ids(raw: &Value) -> SignerVaultFields {
    let signer = raw.get("signer").and_then(Value::as_object);
    let vault = raw.get("vault").and_then(Value::as_object);
    SignerVaultFields {
        signer_kms_key_id: optional_trimmed_str_section(signer, "kms_key_id"),
        signer_kms_region: optional_str_section(signer, "kms_region", "us-west-2"),
        vault_launcher_id: optional_trimmed_str_section(vault, "launcher_id"),
    }
}

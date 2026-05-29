use std::collections::HashMap;
use std::str::FromStr;

use bip39::Mnemonic;
use chia_bls::SecretKey;

use crate::error::{bls_reason, BlsOp, SignerError, SignerResult};

fn env_trimmed(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_secret_key_hex(raw: &str) -> SignerResult<SecretKey> {
    let trimmed = raw.trim();
    let hex_str = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    let bytes = hex::decode(hex_str)
        .map_err(|err| SignerError::Other(format!("invalid_secret_key_hex:{err}")))?;
    let key_bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| SignerError::Other("secret_key_hex_must_be_32_bytes".into()))?;
    SecretKey::from_bytes(&key_bytes)
        .map_err(|err| SignerError::Other(format!("invalid_secret_key_bytes:{err}")))
}

fn parse_json_map(raw: &str, error_label: &str) -> SignerResult<HashMap<String, String>> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|_| SignerError::Other(format!("invalid_{error_label}")))?;
    let Some(object) = value.as_object() else {
        return Err(SignerError::Other(format!("invalid_{error_label}")));
    };
    let mut out = HashMap::new();
    for (key, item) in object {
        if let Some(text) = item.as_str() {
            out.insert(key.clone(), text.to_string());
        }
    }
    Ok(out)
}

fn mnemonic_for_key_id(key_id: &str) -> SignerResult<String> {
    if let Some(raw) = env_trimmed("GREENFLOOR_KEY_ID_MNEMONIC_MAP_JSON") {
        let map = parse_json_map(&raw, "key_id_mnemonic_map_json")?;
        if let Some(value) = map.get(key_id).filter(|value| !value.trim().is_empty()) {
            return Ok(value.trim().to_string());
        }
    }
    for name in ["GREENFLOOR_WALLET_MNEMONIC", "TESTNET_WALLET_MNEMONIC"] {
        if let Some(value) = env_trimmed(name) {
            return Ok(value);
        }
    }
    Err(SignerError::Other("missing_mnemonic_for_key_id".into()))
}

pub fn load_bls_master_secret_key(key_id: &str) -> SignerResult<SecretKey> {
    let key_id = key_id.trim();
    if key_id.is_empty() {
        return Err(SignerError::Other("missing_key_id".into()));
    }

    if let Some(raw) = env_trimmed("GREENFLOOR_KEY_ID_SECRET_KEY_HEX_MAP_JSON") {
        let map = parse_json_map(&raw, "key_id_secret_key_hex_map_json")?;
        if let Some(hex_value) = map.get(key_id) {
            return parse_secret_key_hex(hex_value);
        }
    }

    let mnemonic_text = mnemonic_for_key_id(key_id)?;
    let mnemonic = Mnemonic::from_str(&mnemonic_text)
        .map_err(|err| SignerError::Other(format!("mnemonic_to_master_key_error:{err}")))?;
    let seed = mnemonic.to_seed("");
    Ok(SecretKey::from_seed(&seed))
}

//! Redaction helpers for operator logs and audit payloads mirrored to tracing.

use serde_json::Value;

const SENSITIVE_KEYS: &[&str] = &[
    "offer_text",
    "offer",
    "secret",
    "password",
    "token",
    "private_key",
    "mnemonic",
    "seed",
];

/// Truncate a hex or opaque id for log display.
#[must_use]
pub fn truncate_id(id: &str, visible: usize) -> String {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.len() <= visible {
        return trimmed.to_string();
    }
    format!("{}…", &trimmed[..visible])
}

/// Reference form for Bech32m offer strings — never log full offer bodies.
#[must_use]
pub fn offer_log_ref(offer: &str) -> String {
    let trimmed = offer.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let prefix_len = trimmed.len().min(if trimmed.starts_with("offer1") {
        16
    } else {
        12
    });
    format!("{}…len={}", &trimmed[..prefix_len], trimmed.len())
}

fn redact_string_value(key: &str, value: &str) -> String {
    let normalized = key.trim().to_ascii_lowercase();
    if SENSITIVE_KEYS
        .iter()
        .any(|candidate| normalized.contains(candidate))
    {
        return offer_log_ref(value);
    }
    if normalized.ends_with("_id") || normalized == "coin_id" || normalized == "tx_id" {
        return truncate_id(value, 8);
    }
    if value.starts_with("offer1") && value.len() > 24 {
        return offer_log_ref(value);
    }
    value.to_string()
}

/// Recursively redact sensitive keys in JSON payloads before tracing.
#[must_use]
pub fn redact_json_for_log(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, child) in map {
                if SENSITIVE_KEYS
                    .iter()
                    .any(|candidate| key.to_ascii_lowercase().contains(candidate))
                {
                    if let Value::String(text) = child {
                        out.insert(key.clone(), Value::String(offer_log_ref(text)));
                    } else {
                        out.insert(key.clone(), Value::String("<redacted>".to_string()));
                    }
                    continue;
                }
                if let Value::String(text) = child {
                    out.insert(key.clone(), Value::String(redact_string_value(key, text)));
                    continue;
                }
                out.insert(key.clone(), redact_json_for_log(child));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_json_for_log).collect()),
        Value::String(text) => Value::String(text.clone()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn truncate_id_shortens_long_hex() {
        let id = "a".repeat(64);
        assert_eq!(truncate_id(&id, 8), format!("{}…", "a".repeat(8)));
        assert_eq!(truncate_id("short", 8), "short");
    }

    #[test]
    fn offer_log_ref_never_includes_full_offer() {
        let offer = format!("offer1{}", "q".repeat(200));
        let reference = offer_log_ref(&offer);
        assert!(reference.contains("len="));
        assert!(reference.len() < offer.len());
        assert!(!reference.contains(&"q".repeat(50)));
    }

    #[test]
    fn redact_json_for_log_strips_offer_text() {
        let payload = json!({
            "market_id": "m1",
            "offer_text": format!("offer1{}", "x".repeat(80)),
            "offer_id": "abc1234567890deadbeef",
        });
        let redacted = redact_json_for_log(&payload);
        let offer_text = redacted
            .get("offer_text")
            .and_then(Value::as_str)
            .expect("offer_text");
        assert!(offer_text.contains("len="));
        assert!(!offer_text.contains(&"x".repeat(40)));
    }

    #[test]
    fn redact_json_for_log_truncates_ids_and_masks_secrets() {
        let payload = json!({
            "market_id": "abc1234567890deadbeef",
            "password": "hunter2",
        });
        let redacted = redact_json_for_log(&payload);
        assert_eq!(
            redacted.get("market_id").and_then(Value::as_str),
            Some("abc12345…")
        );
        assert!(redacted
            .get("password")
            .and_then(Value::as_str)
            .is_some_and(|value| value.contains("len=")));
    }
}

#[must_use]
pub fn to_coinset_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

#[must_use]
pub fn u64_from_value(value: Option<&serde_json::Value>, default: u64) -> u64 {
    value
        .and_then(|raw| {
            raw.as_u64()
                .or_else(|| raw.as_i64().and_then(|v| u64::try_from(v).ok()))
        })
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn to_coinset_hex_prefixes_0x() {
        assert_eq!(to_coinset_hex(&[0xab]), "0xab");
    }

    #[test]
    fn u64_from_value_prefers_u64_and_parses_i64() {
        assert_eq!(u64_from_value(Some(&json!(42_u64)), 0), 42);
        assert_eq!(u64_from_value(Some(&json!(7_i64)), 0), 7);
        assert_eq!(u64_from_value(Some(&json!("bad")), 99), 99);
        assert_eq!(u64_from_value(None, 5), 5);
    }
}

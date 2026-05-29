use crate::coinset::is_canonical_xch_asset;

const CANONICAL_XCH_MOJOS: i64 = 1_000_000_000_000;
const CANONICAL_CAT_MOJOS: i64 = 1_000;

/// Return true when *value* is a 64-character lowercase hex string (optional ``0x`` prefix).
pub fn is_hex_id(value: &str) -> bool {
    normalize_hex_id(value).len() == 64
}

/// Normalize a hex identifier: strip, lowercase, remove ``0x`` prefix.
///
/// Returns the 64-char hex string, or empty when invalid.
pub fn normalize_hex_id(value: &str) -> String {
    let mut normalized = value.trim().to_ascii_lowercase();
    if normalized.starts_with("0x") {
        normalized = normalized[2..].to_string();
    }
    if normalized.len() != 64 {
        return String::new();
    }
    if !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return String::new();
    }
    normalized
}

pub fn default_mojo_multiplier_for_asset(asset_id: &str) -> i64 {
    if is_canonical_xch_asset(asset_id) {
        CANONICAL_XCH_MOJOS
    } else {
        CANONICAL_CAT_MOJOS
    }
}

#[cfg(test)]
mod tests {
    use super::{default_mojo_multiplier_for_asset, is_hex_id, normalize_hex_id};

    #[test]
    fn recognizes_valid_hex_ids() {
        let id = "a".repeat(64);
        assert!(is_hex_id(&id));
        assert!(is_hex_id(&format!("0x{id}")));
        assert_eq!(normalize_hex_id(&format!("0X{id}")), id);
    }

    #[test]
    fn rejects_invalid_hex_ids() {
        assert!(!is_hex_id("abc"));
        assert!(!is_hex_id(&"g".repeat(64)));
        assert_eq!(normalize_hex_id("not-hex"), "");
    }

    #[test]
    fn mojo_multiplier_matches_asset_kind() {
        assert_eq!(default_mojo_multiplier_for_asset("xch"), 1_000_000_000_000);
        assert_eq!(
            default_mojo_multiplier_for_asset(
                "0000000000000000000000000000000000000000000000000000000000000001"
            ),
            1_000
        );
    }
}

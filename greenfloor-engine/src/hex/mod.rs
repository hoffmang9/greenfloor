//! Hex normalization, byte parsing, and CLVM/tree-hash encoding helpers.

mod bytes;
mod clvm;

use crate::coinset::is_canonical_xch_asset;

pub use bytes::{fixed_bytes, hex_to_bytes, hex_to_bytes32, parse_coin_ids};
pub use clvm::{bytes32_to_hex, hex_to_tree_hash, tree_hash_nil, tree_hash_to_hex};

const CANONICAL_XCH_MOJOS: i64 = 1_000_000_000_000;
const CANONICAL_CAT_MOJOS: i64 = 1_000;

/// Canonical hex normalization: trim, strip optional ``0x``, lowercase, hex digits only.
#[must_use]
pub fn normalize_hex(value: &str) -> String {
    let mut normalized = value.trim().to_ascii_lowercase();
    if normalized.starts_with("0x") {
        normalized = normalized[2..].to_string();
    }
    normalized.chars().filter(char::is_ascii_hexdigit).collect()
}

/// Return true when *value* is a 64-character lowercase hex string (optional ``0x`` prefix).
#[must_use]
pub fn is_hex_id(value: &str) -> bool {
    normalize_hex_id(value).len() == 64
}

/// Normalize a hex identifier: strip, lowercase, remove ``0x`` prefix.
///
/// Returns the 64-char hex string, or empty when invalid.
#[must_use]
pub fn normalize_hex_id(value: &str) -> String {
    let normalized = normalize_hex(value);
    if normalized.len() != 64 {
        return String::new();
    }
    normalized
}

#[must_use]
pub fn default_mojo_multiplier_for_asset(asset_id: &str) -> i64 {
    if is_canonical_xch_asset(asset_id) {
        CANONICAL_XCH_MOJOS
    } else {
        CANONICAL_CAT_MOJOS
    }
}

#[cfg(test)]
mod tests {
    use super::{default_mojo_multiplier_for_asset, is_hex_id, normalize_hex, normalize_hex_id};

    #[test]
    fn normalize_hex_strips_prefix_and_non_hex() {
        assert_eq!(normalize_hex("0xAb 01"), "ab01");
        assert_eq!(normalize_hex("0Xab01"), "ab01");
    }

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

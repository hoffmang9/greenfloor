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

/// Canonical 64-char lowercase tx/coin id, or `None` when invalid.
#[must_use]
pub fn canonical_tx_id(value: &str) -> Option<String> {
    let normalized = normalize_hex_id(value);
    (!normalized.is_empty()).then_some(normalized)
}

/// Legacy `0x`-prefixed form of a canonical tx id (for tolerant DB lookups).
#[must_use]
pub fn legacy_prefixed_tx_id(canonical: &str) -> Option<String> {
    canonical_tx_id(canonical).map(|id| format!("0x{id}"))
}

/// Canonical and legacy-prefixed tx ids for tolerant sqlite lookups.
#[must_use]
pub fn tx_id_lookup_candidates(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let input_was_prefixed = trimmed
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("0x"));
    let Some(canonical) = canonical_tx_id(value) else {
        return Vec::new();
    };
    let mut out = vec![canonical.clone()];
    if !input_was_prefixed {
        if let Some(legacy) = legacy_prefixed_tx_id(&canonical) {
            if legacy != canonical {
                out.push(legacy);
            }
        }
    }
    out
}

/// Append lookup candidates for *value* into *unique* without duplicates.
pub fn extend_tx_id_lookup_candidates(unique: &mut Vec<String>, value: &str) {
    for candidate in tx_id_lookup_candidates(value) {
        if !unique.iter().any(|existing| existing == &candidate) {
            unique.push(candidate);
        }
    }
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
    use super::{canonical_tx_id, tx_id_lookup_candidates};
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
    fn canonical_tx_id_rejects_invalid() {
        assert!(canonical_tx_id("not-hex").is_none());
    }

    #[test]
    fn tx_id_lookup_candidates_include_canonical_and_legacy() {
        let id = "a".repeat(64);
        assert_eq!(
            tx_id_lookup_candidates(&id),
            vec![id.clone(), format!("0x{id}")]
        );
        assert_eq!(tx_id_lookup_candidates(&format!("0x{id}")), vec![id]);
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

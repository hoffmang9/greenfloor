/// Canonical XCH / TXCH asset identifiers for coinset and BLS paths.
///
/// Empty/whitespace is **not** XCH. Use [`is_xch_like_asset`] at signer payload
/// boundaries where empty means native XCH.
#[must_use]
pub fn is_canonical_xch_asset(asset_id: &str) -> bool {
    matches!(
        asset_id.trim().to_ascii_lowercase().as_str(),
        "xch" | "txch" | "1"
    )
}

#[must_use]
pub fn is_xch_like_asset(asset_id: &str) -> bool {
    asset_id.trim().is_empty() || is_canonical_xch_asset(asset_id)
}

#[cfg(test)]
mod tests {
    use super::{is_canonical_xch_asset, is_xch_like_asset};

    #[test]
    fn recognizes_xch_like_assets() {
        assert!(is_xch_like_asset("xch"));
        assert!(is_xch_like_asset("TXCH"));
        assert!(is_xch_like_asset(""));
        assert!(!is_canonical_xch_asset(""));
        assert!(!is_xch_like_asset(&"aa".repeat(32)));
    }
}

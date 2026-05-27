/// Canonical XCH / TXCH asset identifiers for coinset and BLS paths.
pub fn is_xch_like_asset(asset_id: &str) -> bool {
    matches!(
        asset_id.trim().to_ascii_lowercase().as_str(),
        "" | "xch" | "txch" | "1"
    )
}

#[cfg(test)]
mod tests {
    use super::is_xch_like_asset;

    #[test]
    fn recognizes_xch_like_assets() {
        assert!(is_xch_like_asset("xch"));
        assert!(is_xch_like_asset("TXCH"));
        assert!(!is_xch_like_asset(&"aa".repeat(32)));
    }
}

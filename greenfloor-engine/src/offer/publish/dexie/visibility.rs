#[must_use]
pub(super) fn is_transient_dexie_visibility_404_error(error: &str) -> bool {
    let normalized = error.trim().to_ascii_lowercase();
    (normalized.contains("dexie_get_offer_error") && normalized.contains("404"))
        || normalized.contains("dexie_http_error:404")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dexie_visibility_404_is_transient() {
        assert!(is_transient_dexie_visibility_404_error(
            "dexie_http_error:404 not found"
        ));
        assert!(is_transient_dexie_visibility_404_error(
            "dexie_get_offer_error:404 missing"
        ));
        assert!(!is_transient_dexie_visibility_404_error(
            "dexie_offer_offered_asset_missing:cat"
        ));
    }
}

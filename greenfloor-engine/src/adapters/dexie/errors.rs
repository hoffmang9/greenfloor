//! Dexie error-text classification shared by lifecycle, publish, and stale sweep.

/// Whether Dexie error text means the offer is missing (404 / not found).
#[must_use]
pub fn is_dexie_offer_missing_error_text(error_text: &str) -> bool {
    let normalized = error_text.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    (normalized.contains("dexie_get_offer_error") && normalized.contains("404"))
        || normalized.contains("dexie_http_error:404")
        || (normalized.contains("http error 404") && normalized.contains("not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_dexie_404_shapes() {
        assert!(is_dexie_offer_missing_error_text(
            "dexie_http_error:404 not found"
        ));
        assert!(is_dexie_offer_missing_error_text(
            "dexie_get_offer_error:404 missing"
        ));
        assert!(is_dexie_offer_missing_error_text(
            "HTTP error 404: Not Found"
        ));
        assert!(!is_dexie_offer_missing_error_text(
            "dexie_offer_offered_asset_missing:cat"
        ));
    }
}

use crate::adapters::is_dexie_offer_missing_error_text;

#[must_use]
pub(super) fn is_transient_dexie_visibility_404_error(error: &str) -> bool {
    is_dexie_offer_missing_error_text(error)
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

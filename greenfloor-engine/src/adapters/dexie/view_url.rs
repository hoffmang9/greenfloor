#[must_use]
pub fn dexie_offer_view_url(dexie_base_url: &str, offer_id: &str) -> String {
    let clean_offer_id = offer_id.trim();
    if clean_offer_id.is_empty() {
        return String::new();
    }
    let trimmed = dexie_base_url.trim();
    let host = trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let host = if let Some(rest) = host.strip_prefix("api-testnet.") {
        format!("testnet.{rest}")
    } else if let Some(rest) = host.strip_prefix("api.") {
        rest.to_string()
    } else {
        host.to_string()
    };
    format!(
        "https://{host}/offers/{}",
        urlencoding::encode(clean_offer_id)
    )
}

#[cfg(test)]
mod tests {
    use super::dexie_offer_view_url;

    #[test]
    fn dexie_view_url_strips_api_prefix() {
        assert_eq!(
            dexie_offer_view_url("https://api.dexie.space", "offer-123"),
            "https://dexie.space/offers/offer-123"
        );
        assert_eq!(
            dexie_offer_view_url("https://api-testnet.dexie.space", "offer-123"),
            "https://testnet.dexie.space/offers/offer-123"
        );
    }
}

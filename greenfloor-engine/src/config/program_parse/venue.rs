use serde_json::Value;

use super::super::program::{DEFAULT_DEXIE_API_BASE, DEFAULT_SPLASH_API_BASE};
use super::super::yaml_fields::{config_err, optional_str_section};
use super::helpers::{normalize_api_base, venues_subsection};
use crate::error::SignerResult;

pub(super) struct VenueFields {
    pub dexie_api_base: String,
    pub splash_api_base: String,
    pub offer_publish_venue: String,
}

pub(super) fn parse_venue_config(raw: &Value) -> SignerResult<VenueFields> {
    let offer_publish_venue = optional_str_section(
        venues_subsection(raw, "offer_publish"),
        "provider",
        "coinset",
    )
    .to_ascii_lowercase();
    if offer_publish_venue != "coinset"
        && offer_publish_venue != "dexie"
        && offer_publish_venue != "splash"
    {
        return Err(config_err(
            "venues.offer_publish.provider must be one of: coinset, dexie, splash",
        ));
    }
    Ok(VenueFields {
        dexie_api_base: normalize_api_base(
            venues_subsection(raw, "dexie").and_then(|section| section.get("api_base")),
            DEFAULT_DEXIE_API_BASE,
        ),
        splash_api_base: normalize_api_base(
            venues_subsection(raw, "splash").and_then(|section| section.get("api_base")),
            DEFAULT_SPLASH_API_BASE,
        ),
        offer_publish_venue,
    })
}

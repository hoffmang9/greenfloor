//! Metrics-agnostic Dexie `get_offer` parsing shared by reconcile prepare, augment, and CLI.

use serde_json::Value;

use crate::adapters::DexieClient;
use crate::cycle::is_dexie_offer_missing_error_text;

use super::super::dexie_index::offer_matches_local_id;
use super::super::transition::missing_offer_error_from_payload;

/// Result of a single Dexie `get_offer` lookup, before lifecycle or watch side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DexieOfferFetch {
    /// Offer body (`get_offer.offer` when present and id-matched, else top-level payload).
    Found(Value),
    /// Dexie reported the offer missing (404 body or missing-error text).
    Missing(String),
    /// Response succeeded but did not match the requested local offer id.
    Mismatch,
    /// Transport or non-missing Dexie error.
    LookupError(String),
}

/// Parse a Dexie `get_offer` JSON body for one local offer id.
///
/// Watch-heal paths require an `offer` sub-object whose lookup keys match `offer_id`.
/// Lifecycle paths accept `offer` when present, otherwise the top-level payload.
#[must_use]
pub fn parse_dexie_get_offer_response(
    offer_id: &str,
    payload: &Value,
    require_id_match: bool,
) -> DexieOfferFetch {
    if let Some(error_text) = missing_offer_error_from_payload(payload) {
        return DexieOfferFetch::Missing(error_text);
    }
    if let Some(single) = payload.get("offer") {
        if offer_matches_local_id(single, offer_id) {
            return DexieOfferFetch::Found(single.clone());
        }
        if require_id_match {
            return DexieOfferFetch::Mismatch;
        }
    }
    if require_id_match {
        DexieOfferFetch::Mismatch
    } else {
        DexieOfferFetch::Found(payload.get("offer").unwrap_or(payload).clone())
    }
}

/// Fetch one offer from Dexie and classify the response (no metrics or persist).
pub async fn fetch_dexie_offer(
    dexie: &DexieClient,
    offer_id: &str,
    require_id_match: bool,
) -> DexieOfferFetch {
    match dexie.get_offer(offer_id).await {
        Ok(response) => parse_dexie_get_offer_response(offer_id, response.body(), require_id_match),
        Err(err) if is_dexie_offer_missing_error_text(&err.to_string()) => {
            DexieOfferFetch::Missing(err.to_string())
        }
        Err(err) => DexieOfferFetch::LookupError(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_missing_from_success_false_body() {
        let payload = json!({"success": false, "error": "HTTP Error 404: Not Found"});
        assert_eq!(
            parse_dexie_get_offer_response("ab", &payload, true),
            DexieOfferFetch::Missing("HTTP Error 404: Not Found".into())
        );
    }

    #[test]
    fn parse_found_requires_id_match_for_heal() {
        let offer_id = "ab".repeat(32);
        let payload = json!({"offer": {"id": offer_id.clone(), "status": 1}});
        assert!(matches!(
            parse_dexie_get_offer_response(&offer_id, &payload, true),
            DexieOfferFetch::Found(_)
        ));
    }

    #[test]
    fn parse_mismatch_when_offer_id_differs_and_match_required() {
        let payload = json!({"offer": {"id": "other-id", "status": 1}});
        assert_eq!(
            parse_dexie_get_offer_response("ab".repeat(32).as_str(), &payload, true),
            DexieOfferFetch::Mismatch
        );
    }

    #[test]
    fn parse_lifecycle_accepts_top_level_payload() {
        let offer_id = "offer-ok";
        let payload = json!({"id": offer_id, "status": 4});
        assert!(matches!(
            parse_dexie_get_offer_response(offer_id, &payload, false),
            DexieOfferFetch::Found(_)
        ));
    }
}

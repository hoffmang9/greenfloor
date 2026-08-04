//! Metrics-agnostic Dexie `get_offer` parsing shared by reconcile prepare, augment, and CLI.

use serde_json::Value;

use crate::adapters::DexieClient;
use crate::cycle::is_dexie_offer_missing_error_text;

use super::super::dexie_index::offer_matches_local_id;
use super::super::transition::missing_offer_error_from_payload;

/// Whether a Dexie `get_offer` result must match the requested local offer id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DexieFetchMode {
    /// Watch-heal / augment paths: an id mismatch is a hard failure (produces `Mismatch`).
    HealStrict,
    /// Lifecycle resolve paths: accept any returned payload — Dexie's `get_offer` is keyed
    /// by the id we requested, so a payload it returns is authoritative for that id even
    /// when the payload's own `id` field looks different. Never produces `Mismatch`.
    LifecycleLoose,
}

/// Result of a single Dexie `get_offer` lookup, before lifecycle or watch side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DexieOfferFetch {
    /// Offer body (`get_offer.offer` when present and id-matched, else top-level payload).
    Found(Value),
    /// Dexie reported the offer missing (404 body or missing-error text).
    Missing(String),
    /// Response succeeded but did not match the requested local offer id
    /// (only produced for [`DexieFetchMode::HealStrict`]).
    Mismatch,
    /// Transport or non-missing Dexie error.
    LookupError(String),
}

/// Parse a Dexie `get_offer` JSON body for one local offer id.
///
/// Watch-heal paths (`HealStrict`) require an `offer` sub-object whose lookup keys match
/// `offer_id`. Lifecycle paths (`LifecycleLoose`) accept `offer` when present, otherwise the
/// top-level payload.
#[must_use]
pub fn parse_dexie_get_offer_response(
    offer_id: &str,
    payload: &Value,
    mode: DexieFetchMode,
) -> DexieOfferFetch {
    if let Some(error_text) = missing_offer_error_from_payload(payload) {
        return DexieOfferFetch::Missing(error_text);
    }
    if let Some(single) = payload.get("offer") {
        if offer_matches_local_id(single, offer_id) {
            return DexieOfferFetch::Found(single.clone());
        }
        if mode == DexieFetchMode::HealStrict {
            return DexieOfferFetch::Mismatch;
        }
    }
    match mode {
        DexieFetchMode::HealStrict => DexieOfferFetch::Mismatch,
        DexieFetchMode::LifecycleLoose => {
            DexieOfferFetch::Found(payload.get("offer").unwrap_or(payload).clone())
        }
    }
}

/// Fetch one offer from Dexie and classify the response (no metrics or persist).
pub async fn fetch_dexie_offer(
    dexie: &DexieClient,
    offer_id: &str,
    mode: DexieFetchMode,
) -> DexieOfferFetch {
    match dexie.get_offer(offer_id).await {
        Ok(response) => parse_dexie_get_offer_response(offer_id, response.body(), mode),
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
            parse_dexie_get_offer_response("ab", &payload, DexieFetchMode::HealStrict),
            DexieOfferFetch::Missing("HTTP Error 404: Not Found".into())
        );
    }

    #[test]
    fn parse_found_requires_id_match_for_heal() {
        let offer_id = "ab".repeat(32);
        let payload = json!({"offer": {"id": offer_id.clone(), "status": 1}});
        assert!(matches!(
            parse_dexie_get_offer_response(&offer_id, &payload, DexieFetchMode::HealStrict),
            DexieOfferFetch::Found(_)
        ));
    }

    #[test]
    fn parse_mismatch_when_offer_id_differs_and_match_required() {
        let payload = json!({"offer": {"id": "other-id", "status": 1}});
        assert_eq!(
            parse_dexie_get_offer_response(
                "ab".repeat(32).as_str(),
                &payload,
                DexieFetchMode::HealStrict
            ),
            DexieOfferFetch::Mismatch
        );
    }

    #[test]
    fn parse_lifecycle_accepts_top_level_payload() {
        let offer_id = "offer-ok";
        let payload = json!({"id": offer_id, "status": 4});
        assert!(matches!(
            parse_dexie_get_offer_response(offer_id, &payload, DexieFetchMode::LifecycleLoose),
            DexieOfferFetch::Found(_)
        ));
    }

    #[test]
    fn parse_lifecycle_never_produces_mismatch_on_id_disagreement() {
        let payload = json!({"offer": {"id": "other-id", "status": 1}});
        assert!(matches!(
            parse_dexie_get_offer_response(
                "ab".repeat(32).as_str(),
                &payload,
                DexieFetchMode::LifecycleLoose
            ),
            DexieOfferFetch::Found(_)
        ));
    }
}

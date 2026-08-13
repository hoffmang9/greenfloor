//! Metrics-agnostic Dexie `get_offer` parsing shared by reconcile prepare, augment, and CLI.

use crate::adapters::{DexieClient, DexieResponse};
use crate::error::{OfferError, SignerError, SignerResult};
use crate::offer::dexie_payload::DexieOfferPayload;

use super::super::dexie_index::offer_matches_local_id;

/// Result of a single Dexie `get_offer` lookup, before lifecycle or watch side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DexieOfferFetch {
    /// Nested `offer` object whose lookup keys match the requested local id.
    Found(serde_json::Value),
    /// Dexie reported the offer missing (HTTP 404 or explicit `success: false`).
    Missing(String),
    /// Response succeeded but did not match the requested local offer id.
    Mismatch,
}

fn parse_dexie_get_offer_response(offer_id: &str, response: &DexieResponse) -> DexieOfferFetch {
    if let Some(single) = response.offer_payload() {
        if offer_matches_local_id(single, offer_id) {
            return DexieOfferFetch::Found(single.clone());
        }
    }
    if response.is_explicit_failure() {
        return DexieOfferFetch::Missing(response.error_text().to_string());
    }
    DexieOfferFetch::Mismatch
}

/// Fetch one offer (id mismatch is [`DexieOfferFetch::Mismatch`]).
///
/// Transport failures propagate as [`SignerError`]; HTTP 404 is [`DexieOfferFetch::Missing`].
pub async fn fetch_dexie_offer(
    dexie: &DexieClient,
    offer_id: &str,
) -> SignerResult<DexieOfferFetch> {
    match dexie.get_offer(offer_id).await {
        Ok(response) => Ok(parse_dexie_get_offer_response(offer_id, &response)),
        Err(err) if err.is_http_not_found() => Ok(DexieOfferFetch::Missing(err.to_string())),
        Err(err) => Err(err),
    }
}

/// Fetch Dexie offer-file text for cancel fallback.
///
/// Trusts the URL-fetched body (no local-id match required). Transport errors propagate.
pub async fn fetch_dexie_offer_file_text(
    dexie: &DexieClient,
    offer_id: &str,
) -> SignerResult<String> {
    let response = match dexie.get_offer(offer_id).await {
        Ok(response) => response,
        Err(err) if err.is_http_not_found() => {
            return Err(SignerError::Offer(OfferError::OfferCancelOfferFileNotFound));
        }
        Err(err) => return Err(err),
    };
    if response.is_explicit_failure() {
        return Err(SignerError::Offer(OfferError::OfferCancelOfferFileNotFound));
    }
    if let Some(text) = DexieOfferPayload::new(response.body().clone()).offer_file_text() {
        return Ok(text.to_string());
    }
    Err(SignerError::Offer(OfferError::OfferCancelOfferFileMissing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{SignerError, TransportError};
    use serde_json::json;

    #[test]
    fn parse_missing_from_success_false_body() {
        let response = DexieResponse::from_value(json!({
            "success": false,
            "error": "HTTP Error 404: Not Found"
        }));
        assert_eq!(
            parse_dexie_get_offer_response("ab", &response),
            DexieOfferFetch::Missing("HTTP Error 404: Not Found".into())
        );
    }

    #[test]
    fn parse_found_requires_id_match() {
        let offer_id = "ab".repeat(32);
        let response = DexieResponse::from_value(json!({
            "offer": {"id": offer_id.clone(), "status": 1}
        }));
        assert!(matches!(
            parse_dexie_get_offer_response(&offer_id, &response),
            DexieOfferFetch::Found(_)
        ));
    }

    #[test]
    fn parse_mismatch_when_offer_id_differs() {
        let response = DexieResponse::from_value(json!({"offer": {"id": "other-id", "status": 1}}));
        assert_eq!(
            parse_dexie_get_offer_response("ab".repeat(32).as_str(), &response),
            DexieOfferFetch::Mismatch
        );
    }

    #[test]
    fn parse_mismatch_on_top_level_payload_without_nested_offer() {
        let offer_id = "offer-ok";
        let response = DexieResponse::from_value(json!({"id": offer_id, "status": 4}));
        assert_eq!(
            parse_dexie_get_offer_response(offer_id, &response),
            DexieOfferFetch::Mismatch
        );
    }

    #[tokio::test]
    async fn fetch_offer_maps_http_404_to_missing() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/offers/missing")
            .with_status(404)
            .with_body(r#"{"success":false,"error":"not_found"}"#)
            .create();
        let dexie = DexieClient::new(server.url());
        let fetch = fetch_dexie_offer(&dexie, "missing").await.expect("fetch");
        assert!(matches!(fetch, DexieOfferFetch::Missing(_)));
    }

    #[tokio::test]
    async fn fetch_offer_file_text_extracts_without_id_match() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/offers/local-id")
            .with_status(200)
            .with_body(r#"{"offer":{"id":"other-id","offer":"offer1qq"}}"#)
            .create();
        let dexie = DexieClient::new(server.url());
        let text = fetch_dexie_offer_file_text(&dexie, "local-id")
            .await
            .expect("file");
        assert_eq!(text, "offer1qq");
    }

    #[tokio::test]
    async fn fetch_offer_file_text_maps_404_to_not_found() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/offers/missing")
            .with_status(404)
            .with_body(r#"{"success":false,"error":"not_found"}"#)
            .create();
        let dexie = DexieClient::new(server.url());
        let err = fetch_dexie_offer_file_text(&dexie, "missing")
            .await
            .expect_err("404");
        assert!(matches!(
            err,
            SignerError::Offer(OfferError::OfferCancelOfferFileNotFound)
        ));
    }

    #[tokio::test]
    async fn fetch_offer_file_text_maps_present_body_without_file() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/offers/no-file")
            .with_status(200)
            .with_body(r#"{"offer":{"id":"no-file","status":1}}"#)
            .create();
        let dexie = DexieClient::new(server.url());
        let err = fetch_dexie_offer_file_text(&dexie, "no-file")
            .await
            .expect_err("missing file");
        assert!(matches!(
            err,
            SignerError::Offer(OfferError::OfferCancelOfferFileMissing)
        ));
    }

    #[tokio::test]
    async fn fetch_offer_file_text_propagates_transport() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/offers/bad-json")
            .with_status(200)
            .with_body("not-json")
            .create();
        let dexie = DexieClient::new(server.url());
        let err = fetch_dexie_offer_file_text(&dexie, "bad-json")
            .await
            .expect_err("transport");
        assert!(matches!(
            err,
            SignerError::Transport(TransportError::Http {
                layer: "dexie_json_error",
                ..
            })
        ));
    }
}

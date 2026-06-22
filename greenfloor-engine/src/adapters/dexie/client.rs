use serde_json::{json, Value};

use super::response::DexieResponse;
use crate::adapters::http_json::{self, AdapterResponseTags};
use crate::error::{SignerError, SignerResult};

const RESPONSE_TAGS: AdapterResponseTags = AdapterResponseTags {
    http_error_prefix: "dexie_http_error",
    json_error_prefix: "dexie_json_error",
    read_error_prefix: "dexie_read_error",
};

#[derive(Debug, Clone)]
pub struct DexieClient {
    base_url: String,
    http: reqwest::Client,
}

impl DexieClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Post offer.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn post_offer(
        &self,
        offer: &str,
        drop_only: bool,
        claim_rewards: bool,
    ) -> SignerResult<DexieResponse> {
        self.post_json(
            "/v1/offers",
            json!({
                "offer": offer,
                "drop_only": drop_only,
                "claim_rewards": claim_rewards,
            }),
            20,
            "dexie_network_error",
        )
        .await
        .map(DexieResponse::from_value)
    }

    /// Get offer.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn get_offer(&self, offer_id: &str) -> SignerResult<DexieResponse> {
        let clean_offer_id = offer_id.trim();
        if clean_offer_id.is_empty() {
            return Err(SignerError::Other("offer_id is required".to_string()));
        }
        let encoded = urlencoding::encode(clean_offer_id);
        self.get_json(
            &format!("/v1/offers/{encoded}"),
            20,
            "dexie_get_offer_error",
        )
        .await
        .map(DexieResponse::from_value)
    }

    /// Get offers.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn get_offers(&self, offered: &str, requested: &str) -> SignerResult<Vec<Value>> {
        let query = format!(
            "offered={}&requested={}",
            urlencoding::encode(offered.trim()),
            urlencoding::encode(requested.trim())
        );
        let payload = self
            .get_json(&format!("/v1/offers?{query}"), 20, "dexie_get_offers_error")
            .await?;
        let offers = payload
            .get("offers")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(offers)
    }

    /// Get swap tokens.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn get_swap_tokens(&self) -> SignerResult<Vec<Value>> {
        let payload = self
            .get_json("/v1/swap/tokens", 15, "dexie_get_tokens_error")
            .await?;
        Ok(object_rows_from_payload(&payload, "tokens"))
    }

    /// Get price tickers.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn get_price_tickers(&self) -> SignerResult<Vec<Value>> {
        let payload = self
            .get_json("/v3/prices/tickers", 20, "dexie_get_tickers_error")
            .await?;
        if payload.is_array() {
            return Ok(payload
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(Value::is_object)
                .collect());
        }
        Ok(object_rows_from_payload(&payload, "tickers"))
    }

    async fn get_json(
        &self,
        path: &str,
        timeout_secs: u64,
        network_err_tag: &str,
    ) -> SignerResult<Value> {
        http_json::get_json(
            &self.http,
            &format!("{}{path}", self.base_url),
            timeout_secs,
            network_err_tag,
            RESPONSE_TAGS,
        )
        .await
    }

    async fn post_json(
        &self,
        path: &str,
        body: Value,
        timeout_secs: u64,
        network_err_tag: &str,
    ) -> SignerResult<Value> {
        http_json::post_json(
            &self.http,
            &format!("{}{path}", self.base_url),
            body,
            timeout_secs,
            network_err_tag,
            RESPONSE_TAGS,
        )
        .await
    }

    #[cfg(test)]
    fn parse_response_body(status: reqwest::StatusCode, body: &str) -> SignerResult<Value> {
        http_json::parse_response_body(status, body, RESPONSE_TAGS)
    }
}

fn object_rows_from_payload(payload: &Value, array_key: &str) -> Vec<Value> {
    let rows = payload
        .get(array_key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| payload.as_array().cloned().unwrap_or_default());
    rows.into_iter().filter(Value::is_object).collect()
}

#[cfg(test)]
mod tests {
    use super::DexieClient;
    use reqwest::StatusCode;

    #[test]
    fn parse_response_body_success_json() {
        let payload =
            DexieClient::parse_response_body(StatusCode::OK, r#"{"success":true,"id":"offer-1"}"#)
                .expect("parse");
        assert_eq!(
            payload.get("success").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(payload.get("id").and_then(|v| v.as_str()), Some("offer-1"));
    }

    #[test]
    fn parse_response_body_http_error_returns_success_false() {
        let payload =
            DexieClient::parse_response_body(StatusCode::NOT_FOUND, "missing").expect("parse");
        assert_eq!(
            payload.get("success").and_then(serde_json::Value::as_bool),
            Some(false)
        );
        assert_eq!(
            payload.get("error").and_then(|v| v.as_str()),
            Some("dexie_http_error:404:missing")
        );
    }

    #[test]
    fn parse_response_body_invalid_json_is_err() {
        let err = DexieClient::parse_response_body(StatusCode::OK, "not-json").unwrap_err();
        assert!(err.to_string().contains("dexie_json_error"));
    }
}

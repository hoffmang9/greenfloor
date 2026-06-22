use serde_json::{json, Value};

use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone)]
pub struct DexieClient {
    pub(super) base_url: String,
    pub(super) http: reqwest::Client,
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
    ) -> SignerResult<Value> {
        let payload = json!({
            "offer": offer,
            "drop_only": drop_only,
            "claim_rewards": claim_rewards,
        });
        let url = format!("{}/v1/offers", self.base_url);
        let response = self
            .http
            .post(url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .map_err(|err| SignerError::Other(format!("dexie_network_error:{err}")))?;
        Self::parse_response(response).await
    }

    /// Get offer.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn get_offer(&self, offer_id: &str) -> SignerResult<Value> {
        let clean_offer_id = offer_id.trim();
        if clean_offer_id.is_empty() {
            return Err(SignerError::Other("offer_id is required".to_string()));
        }
        let encoded = urlencoding::encode(clean_offer_id);
        let url = format!("{}/v1/offers/{encoded}", self.base_url);
        let response = self
            .http
            .get(url)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .map_err(|err| SignerError::Other(format!("dexie_get_offer_error:{err}")))?;
        Self::parse_response(response).await
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
        let url = format!("{}/v1/offers?{query}", self.base_url);
        let response = self
            .http
            .get(url)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .map_err(|err| SignerError::Other(format!("dexie_get_offers_error:{err}")))?;
        let payload = Self::parse_response(response).await?;
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
        let url = format!("{}/v1/swap/tokens", self.base_url);
        let response = self
            .http
            .get(url)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|err| SignerError::Other(format!("dexie_get_tokens_error:{err}")))?;
        let payload = Self::parse_response(response).await?;
        let tokens = payload
            .get("tokens")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| payload.as_array().cloned().unwrap_or_default());
        Ok(tokens
            .into_iter()
            .filter(serde_json::Value::is_object)
            .collect())
    }

    /// Get price tickers.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn get_price_tickers(&self) -> SignerResult<Vec<Value>> {
        let url = format!("{}/v3/prices/tickers", self.base_url);
        let response = self
            .http
            .get(url)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .map_err(|err| SignerError::Other(format!("dexie_get_tickers_error:{err}")))?;
        let payload = Self::parse_response(response).await?;
        if let Some(rows) = payload.as_array() {
            return Ok(rows.iter().filter(|row| row.is_object()).cloned().collect());
        }
        Ok(payload
            .get("tickers")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(serde_json::Value::is_object)
            .collect())
    }

    /// Cancel offer.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn cancel_offer(&self, offer_id: &str) -> SignerResult<Value> {
        let clean_offer_id = offer_id.trim();
        if clean_offer_id.is_empty() {
            return Err(SignerError::Other("offer_id is required".to_string()));
        }
        let encoded = urlencoding::encode(clean_offer_id);
        let url = format!("{}/v1/offers/{encoded}/cancel", self.base_url);
        let response = self
            .http
            .post(url)
            .json(&json!({"id": clean_offer_id}))
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .map_err(|err| SignerError::Other(format!("dexie_cancel_offer_error:{err}")))?;
        Self::parse_response(response).await
    }

    pub(super) async fn parse_response(response: reqwest::Response) -> SignerResult<Value> {
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| SignerError::Other(format!("dexie_read_error:{err}")))?;
        if !status.is_success() {
            let snippet: String = body.chars().take(500).collect();
            let error = if snippet.is_empty() {
                format!("dexie_http_error:{}", status.as_u16())
            } else {
                format!("dexie_http_error:{}:{snippet}", status.as_u16())
            };
            return Ok(json!({"success": false, "error": error}));
        }
        serde_json::from_str(&body)
            .map_err(|err| SignerError::Other(format!("dexie_json_error:{err}")))
    }
}

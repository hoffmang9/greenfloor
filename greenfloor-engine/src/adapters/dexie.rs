use serde_json::{json, Value};

use crate::cycle::{
    dexie_invalid_offer_retry_sleep, dexie_invalid_offer_should_retry,
    is_transient_dexie_visibility_404_error,
};
use crate::error::{SignerError, SignerResult};
use crate::offer::publish::dexie_offer_asset_expectation_error;

const INVALID_OFFER_RETRY_MAX_ATTEMPTS: u32 = 4;
const INVALID_OFFER_RETRY_INITIAL_SLEEP_SECONDS: f64 = 1.0;
const VISIBILITY_POST_MAX_ATTEMPTS: u32 = 3;
const VISIBILITY_POST_DELAY_SECONDS: f64 = 2.0;

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

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

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

    async fn parse_response(response: reqwest::Response) -> SignerResult<Value> {
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

pub async fn post_dexie_offer_with_invalid_offer_retry(
    dexie: &DexieClient,
    offer_text: &str,
    drop_only: bool,
    claim_rewards: bool,
) -> SignerResult<Value> {
    let mut attempt = 0u32;
    loop {
        let result = dexie
            .post_offer(offer_text, drop_only, claim_rewards)
            .await?;
        let error = result
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if !dexie_invalid_offer_should_retry(
            &error,
            attempt,
            INVALID_OFFER_RETRY_MAX_ATTEMPTS,
        ) {
            return Ok(result);
        }
        let sleep_seconds = dexie_invalid_offer_retry_sleep(
            attempt,
            INVALID_OFFER_RETRY_INITIAL_SLEEP_SECONDS,
        );
        tokio::time::sleep(std::time::Duration::from_secs_f64(sleep_seconds)).await;
        attempt += 1;
    }
}

pub async fn verify_dexie_offer_visible_by_id(
    dexie: &DexieClient,
    offer_id: &str,
    expected_offered_asset_id: &str,
    expected_offered_symbol: &str,
    expected_requested_asset_id: &str,
    expected_requested_symbol: &str,
) -> Option<String> {
    let clean_offer_id = offer_id.trim();
    if clean_offer_id.is_empty() {
        return Some("dexie_offer_missing_id_after_publish".to_string());
    }
    let attempts = 4usize;
    let delay_seconds = 1.5;
    let mut last_error = "dexie_offer_not_visible_after_publish".to_string();
    for attempt in 1..=attempts {
        let payload = match dexie.get_offer(clean_offer_id).await {
            Ok(payload) => payload,
            Err(err) => {
                last_error = format!("dexie_get_offer_error:{err}");
                if attempt < attempts {
                    tokio::time::sleep(std::time::Duration::from_secs_f64(delay_seconds)).await;
                }
                continue;
            }
        };
        let offer_payload = payload.get("offer");
        let visible_id = offer_payload
            .and_then(Value::as_object)
            .and_then(|obj| obj.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if visible_id == clean_offer_id {
            if let Some(offer_obj) = offer_payload.and_then(Value::as_object) {
                if let Some(asset_error) = dexie_offer_asset_expectation_error(
                    offer_obj.get("offered").unwrap_or(&Value::Null),
                    offer_obj.get("requested").unwrap_or(&Value::Null),
                    expected_offered_asset_id,
                    expected_offered_symbol,
                    expected_requested_asset_id,
                    expected_requested_symbol,
                ) {
                    return Some(asset_error);
                }
            }
            return None;
        }
        last_error = "dexie_offer_visibility_payload_mismatch".to_string();
        if attempt < attempts {
            tokio::time::sleep(std::time::Duration::from_secs_f64(delay_seconds)).await;
        }
    }
    Some(last_error)
}

pub async fn post_offer_phase_dexie(
    dexie: &DexieClient,
    offer_text: &str,
    drop_only: bool,
    claim_rewards: bool,
    expected_offered_asset_id: &str,
    expected_offered_symbol: &str,
    expected_requested_asset_id: &str,
    expected_requested_symbol: &str,
) -> SignerResult<Value> {
    let mut last_result = json!({"success": false, "error": "dexie_offer_not_visible_after_publish"});
    let mut last_visibility_error = String::new();
    for attempt in 1..=VISIBILITY_POST_MAX_ATTEMPTS {
        let result = post_dexie_offer_with_invalid_offer_retry(
            dexie,
            offer_text,
            drop_only,
            claim_rewards,
        )
        .await?;
        last_result = result.clone();
        if result.get("success").and_then(Value::as_bool) != Some(true) {
            return Ok(result);
        }
        let posted_offer_id = result
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if let Some(visibility_error) = verify_dexie_offer_visible_by_id(
            dexie,
            &posted_offer_id,
            expected_offered_asset_id,
            expected_offered_symbol,
            expected_requested_asset_id,
            expected_requested_symbol,
        )
        .await
        {
            last_visibility_error = visibility_error;
            if !is_transient_dexie_visibility_404_error(&last_visibility_error) {
                let mut failed = result;
                if let Value::Object(obj) = &mut failed {
                    obj.insert("success".to_string(), Value::Bool(false));
                    obj.insert("error".to_string(), Value::String(last_visibility_error.clone()));
                }
                return Ok(failed);
            }
            if attempt < VISIBILITY_POST_MAX_ATTEMPTS {
                tokio::time::sleep(std::time::Duration::from_secs_f64(
                    VISIBILITY_POST_DELAY_SECONDS,
                ))
                .await;
            }
            continue;
        }
        return Ok(result);
    }
    let mut failed = last_result;
    if let Value::Object(obj) = &mut failed {
        obj.insert("success".to_string(), Value::Bool(false));
        obj.insert(
            "error".to_string(),
            Value::String(if last_visibility_error.is_empty() {
                "dexie_offer_not_visible_after_publish".to_string()
            } else {
                last_visibility_error
            }),
        );
    }
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

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

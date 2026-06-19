use serde_json::{json, Value};

use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone)]
pub struct SplashClient {
    base_url: String,
    http: reqwest::Client,
}

impl SplashClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Post offer.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn post_offer(&self, offer: &str) -> SignerResult<Value> {
        let payload = json!({"offer": offer});
        let response = self
            .http
            .post(&self.base_url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|err| SignerError::Other(format!("splash_network_error:{err}")))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| SignerError::Other(format!("splash_read_error:{err}")))?;
        if !status.is_success() {
            return Ok(json!({
                "success": false,
                "error": format!("splash_http_error:{}", status.as_u16())
            }));
        }
        serde_json::from_str(&body)
            .map_err(|err| SignerError::Other(format!("splash_json_error:{err}")))
    }
}

mod response;

use serde_json::json;

use crate::adapters::http_json::{self, AdapterResponseTags};
use crate::error::SignerResult;

pub use response::SplashResponse;

const RESPONSE_TAGS: AdapterResponseTags = AdapterResponseTags {
    http_error_prefix: "splash_http_error",
    json_error_prefix: "splash_json_error",
    read_error_prefix: "splash_read_error",
};

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
    pub async fn post_offer(&self, offer: &str) -> SignerResult<SplashResponse> {
        http_json::post_json(
            &self.http,
            &self.base_url,
            json!({"offer": offer}),
            30,
            "splash_network_error",
            RESPONSE_TAGS,
        )
        .await
        .map(SplashResponse::from_value)
    }
}

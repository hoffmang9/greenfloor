use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_field_names)]
pub(crate) struct AdapterResponseTags {
    pub http_error_prefix: &'static str,
    pub json_error_prefix: &'static str,
    pub read_error_prefix: &'static str,
}

pub(crate) async fn get_json(
    http: &Client,
    url: &str,
    timeout_secs: u64,
    network_err_tag: &str,
    tags: AdapterResponseTags,
) -> SignerResult<Value> {
    let response = http
        .get(url)
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|err| SignerError::Other(format!("{network_err_tag}:{err}")))?;
    parse_response(response, tags).await
}

pub(crate) async fn post_json(
    http: &Client,
    url: &str,
    body: Value,
    timeout_secs: u64,
    network_err_tag: &str,
    tags: AdapterResponseTags,
) -> SignerResult<Value> {
    let response = http
        .post(url)
        .json(&body)
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|err| SignerError::Other(format!("{network_err_tag}:{err}")))?;
    parse_response(response, tags).await
}

async fn parse_response(
    response: reqwest::Response,
    tags: AdapterResponseTags,
) -> SignerResult<Value> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| SignerError::Other(format!("{}:{err}", tags.read_error_prefix)))?;
    parse_response_body(status, &body, tags)
}

pub(crate) fn parse_response_body(
    status: StatusCode,
    body: &str,
    tags: AdapterResponseTags,
) -> SignerResult<Value> {
    if !status.is_success() {
        let snippet: String = body.chars().take(500).collect();
        let error = if snippet.is_empty() {
            format!("{}:{}", tags.http_error_prefix, status.as_u16())
        } else {
            format!("{}:{}:{snippet}", tags.http_error_prefix, status.as_u16())
        };
        return Ok(json!({"success": false, "error": error}));
    }
    serde_json::from_str(body)
        .map_err(|err| SignerError::Other(format!("{}:{err}", tags.json_error_prefix)))
}

#[cfg(test)]
mod tests {
    use super::{parse_response_body, AdapterResponseTags};
    use reqwest::StatusCode;

    const TAGS: AdapterResponseTags = AdapterResponseTags {
        http_error_prefix: "test_http_error",
        json_error_prefix: "test_json_error",
        read_error_prefix: "test_read_error",
    };

    #[test]
    fn parse_response_body_success_json() {
        let payload =
            parse_response_body(StatusCode::OK, r#"{"success":true,"id":"offer-1"}"#, TAGS)
                .expect("parse");
        assert_eq!(
            payload.get("success").and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn parse_response_body_http_error_returns_success_false() {
        let payload = parse_response_body(StatusCode::NOT_FOUND, "missing", TAGS).expect("parse");
        assert_eq!(
            payload.get("error").and_then(|v| v.as_str()),
            Some("test_http_error:404:missing")
        );
    }
}

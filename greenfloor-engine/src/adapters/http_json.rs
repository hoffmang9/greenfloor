use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde_json::Value;

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
    network_err_tag: &'static str,
    tags: AdapterResponseTags,
) -> SignerResult<Value> {
    let response = http
        .get(url)
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|err| SignerError::from_reqwest(network_err_tag, &err))?;
    parse_response(response, tags).await
}

pub(crate) async fn post_json(
    http: &Client,
    url: &str,
    body: Value,
    timeout_secs: u64,
    network_err_tag: &'static str,
    tags: AdapterResponseTags,
) -> SignerResult<Value> {
    let response = http
        .post(url)
        .json(&body)
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|err| SignerError::from_reqwest(network_err_tag, &err))?;
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
        .map_err(|err| SignerError::from_reqwest(tags.read_error_prefix, &err))?;
    parse_response_body(status, &body, tags)
}

pub(crate) fn parse_response_body(
    status: StatusCode,
    body: &str,
    tags: AdapterResponseTags,
) -> SignerResult<Value> {
    if !status.is_success() {
        let snippet: String = body.chars().take(500).collect();
        return Err(SignerError::http_status(
            tags.http_error_prefix,
            status.as_u16(),
            snippet,
        ));
    }
    serde_json::from_str(body)
        .map_err(|err| SignerError::http(tags.json_error_prefix, err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{parse_response_body, AdapterResponseTags};
    use crate::error::{SignerError, TransportError};
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
    fn parse_response_body_http_error_is_typed_status() {
        let err = parse_response_body(StatusCode::NOT_FOUND, "missing", TAGS).expect_err("404");
        assert!(matches!(
            err,
            SignerError::Transport(TransportError::HttpStatus {
                layer: "test_http_error",
                status: 404,
                ..
            })
        ));
        assert!(err.is_http_not_found());
    }
}

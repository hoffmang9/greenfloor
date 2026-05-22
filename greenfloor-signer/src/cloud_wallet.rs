use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::sign::Signer;
use rand::distr::{Alphanumeric, SampleString};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::json;

use crate::config::CloudWalletConfig;
use crate::error::{SignerError, SignerResult};
use crate::vault::context::VaultCustodySnapshot;

const CUSTODY_SNAPSHOT_QUERY: &str = r#"
query getVaultCustodySnapshot($walletId: ID!, $first: Int!) {
  wallet(id: $walletId) {
    custodyConfig {
      vaultCustodyConfig {
        vaultLauncherId
        custodyThreshold
        recoveryThreshold
        recoveryClawbackTimelock
        custodyKeys(first: $first) {
          edges {
            node {
              publicKey
              curve
            }
          }
        }
        recoveryKeys(first: $first) {
          edges {
            node {
              publicKey
              curve
            }
          }
        }
      }
    }
  }
}
"#;

pub struct CloudWalletClient {
    config: CloudWalletConfig,
    http: reqwest::Client,
}

impl CloudWalletClient {
    pub fn new(config: CloudWalletConfig) -> SignerResult<Self> {
        let http = reqwest::Client::builder()
            .build()
            .map_err(|err| SignerError::Other(format!("failed to build HTTP client: {err}")))?;
        Ok(Self { config, http })
    }

    pub async fn get_vault_custody_snapshot(&self) -> SignerResult<VaultCustodySnapshot> {
        let payload = self
            .graphql(
                CUSTODY_SNAPSHOT_QUERY,
                json!({
                    "walletId": self.config.vault_id,
                    "first": 50,
                }),
            )
            .await?;
        let wallet = payload
            .get("wallet")
            .and_then(|value| value.as_object())
            .ok_or(SignerError::VaultSnapshotUnavailable)?;
        let vault_cfg = wallet
            .get("custodyConfig")
            .and_then(|value| value.get("vaultCustodyConfig"))
            .ok_or(SignerError::VaultSnapshotUnavailable)?;
        VaultCustodySnapshot::from_graphql(vault_cfg)
    }

    async fn graphql(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> SignerResult<serde_json::Map<String, serde_json::Value>> {
        let body = json!({ "query": query, "variables": variables });
        let raw_body = serde_json::to_string(&body)
            .map_err(|err| SignerError::Graphql(format!("encode request failed: {err}")))?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        for (name, value) in self.build_auth_headers(&raw_body)? {
            headers.insert(name, value);
        }
        let url = format!("{}/graphql", self.config.base_url);
        let response = self
            .http
            .post(url)
            .headers(headers)
            .body(raw_body)
            .send()
            .await
            .map_err(|err| SignerError::Graphql(format!("request failed: {err}")))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| SignerError::Graphql(format!("read body failed: {err}")))?;
        if !status.is_success() {
            return Err(SignerError::Graphql(format!(
                "HTTP {status}: {text}"
            )));
        }
        let payload: GraphqlResponse = serde_json::from_str(&text).map_err(|err| {
            SignerError::Graphql(format!("decode response failed: {err}; body={text}"))
        })?;
        if let Some(errors) = payload.errors {
            let message = errors
                .into_iter()
                .filter_map(|entry| entry.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(SignerError::Graphql(message));
        }
        payload
            .data
            .ok_or_else(|| SignerError::Graphql("missing data".to_string()))
    }

    fn build_auth_headers(&self, raw_body: &str) -> SignerResult<Vec<(reqwest::header::HeaderName, HeaderValue)>> {
        let nonce = random_nonce(10);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| SignerError::Other(format!("system clock before epoch: {err}")))?
            .as_millis()
            .to_string();
        let canonical = format!("{raw_body}{nonce}{timestamp}");
        let signature = sign_canonical(&self.config.private_key_pem_path, &canonical)?;
        Ok(vec![
            (
                reqwest::header::HeaderName::from_static("chia-user-key-id"),
                HeaderValue::from_str(&self.config.user_key_id).map_err(|err| {
                    SignerError::Other(format!("invalid user key id header: {err}"))
                })?,
            ),
            (
                reqwest::header::HeaderName::from_static("chia-signature"),
                HeaderValue::from_str(&signature).map_err(|err| {
                    SignerError::Other(format!("invalid signature header: {err}"))
                })?,
            ),
            (
                reqwest::header::HeaderName::from_static("chia-nonce"),
                HeaderValue::from_str(&nonce).map_err(|err| {
                    SignerError::Other(format!("invalid nonce header: {err}"))
                })?,
            ),
            (
                reqwest::header::HeaderName::from_static("chia-timestamp"),
                HeaderValue::from_str(&timestamp).map_err(|err| {
                    SignerError::Other(format!("invalid timestamp header: {err}"))
                })?,
            ),
        ])
    }
}

#[derive(Debug, Deserialize)]
struct GraphqlResponse {
    data: Option<serde_json::Map<String, serde_json::Value>>,
    errors: Option<Vec<GraphqlErrorEntry>>,
}

#[derive(Debug, Deserialize)]
struct GraphqlErrorEntry {
    message: Option<String>,
}

fn sign_canonical(pem_path: &std::path::Path, canonical: &str) -> SignerResult<String> {
    let pem = std::fs::read(pem_path).map_err(|err| {
        SignerError::Other(format!(
            "failed to read cloud wallet PEM {}: {err}",
            pem_path.display()
        ))
    })?;
    let key = PKey::private_key_from_pem(&pem).map_err(|err| {
        SignerError::Other(format!("failed to parse cloud wallet PEM: {err}"))
    })?;
    let mut signer = Signer::new(MessageDigest::sha256(), &key).map_err(|err| {
        SignerError::Other(format!("failed to create signer: {err}"))
    })?;
    signer
        .update(canonical.as_bytes())
        .map_err(|err| SignerError::Other(format!("failed to update signer: {err}")))?;
    let signature = signer
        .sign_to_vec()
        .map_err(|err| SignerError::Other(format!("cloud wallet signature failed: {err}")))?;
    Ok(BASE64.encode(signature))
}

fn random_nonce(length: usize) -> String {
    Alphanumeric.sample_string(&mut rand::rng(), length)
}

//! Retry-backed direct Coinset HTTP client for script-style JSON RPC.

use serde_json::{json, Value};

use super::api::{post_coinset_coin_records, post_coinset_record};
use super::direct_api::resolve_direct_client;
use super::parse::{chunk_values, coin_id_from_record, to_coinset_hex, u64_from_value};
use super::retry::with_script_retries;
use crate::error::SignerResult;
use crate::hex::hex_to_bytes32;

fn apply_height_fields(body: &mut Value, start_height: Option<u64>, end_height: Option<u64>) {
    if let Some(obj) = body.as_object_mut() {
        if let Some(start_height) = start_height {
            obj.insert("start_height".to_string(), json!(start_height));
        }
        if let Some(end_height) = end_height {
            obj.insert("end_height".to_string(), json!(end_height));
        }
    }
}

#[derive(Debug, Clone)]
pub struct DirectCoinsetScanClient {
    pub network: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedDirectScanClient {
    pub network: String,
    pub base_url: String,
    pub client: DirectCoinsetScanClient,
}

impl DirectCoinsetScanClient {
    pub fn new(network: &str, base_url: Option<&str>) -> Self {
        Self {
            network: network.trim().to_string(),
            base_url: base_url
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        }
    }

    /// Resolve normalized network/base URL and build a direct scan client.
    #[must_use]
    pub fn resolve(network: &str, base_url: Option<&str>) -> ResolvedDirectScanClient {
        let resolved = resolve_direct_client(network, base_url);
        let network = resolved.network.to_string();
        let base_url = resolved.base_url.clone();
        let client = Self::new(resolved.network, Some(resolved.base_url.as_str()));
        ResolvedDirectScanClient {
            network,
            base_url,
            client,
        }
    }

    async fn coin_records(
        &self,
        endpoint: &str,
        mut body: Value,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        apply_height_fields(&mut body, start_height, end_height);
        post_coinset_coin_records(&self.network, self.base_url.as_deref(), endpoint, body).await
    }

    async fn record(&self, endpoint: &str, body: Value, key: &str) -> SignerResult<Option<Value>> {
        let network = self.network.clone();
        let base_url = self.base_url.clone();
        let key = key.to_string();
        with_script_retries(|| async {
            post_coinset_record(&network, base_url.as_deref(), endpoint, body.clone(), &key).await
        })
        .await
    }

    /// Chain peak height.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn chain_peak_height(&self) -> SignerResult<Option<u64>> {
        let Some(state) = self
            .record("get_blockchain_state", json!({}), "blockchain_state")
            .await?
        else {
            return Ok(None);
        };
        if let Some(peak) = state.get("peak").and_then(Value::as_object) {
            let height = u64_from_value(peak.get("height"), u64::MAX);
            if height != u64::MAX {
                return Ok(Some(height));
            }
        }
        let height = u64_from_value(state.get("peak_height"), u64::MAX);
        if height == u64::MAX {
            Ok(None)
        } else {
            Ok(Some(height))
        }
    }

    /// By puzzle hashes.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn by_puzzle_hashes(
        &self,
        puzzle_hashes: &[String],
        include_spent: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        if puzzle_hashes.is_empty() {
            return Ok(Vec::new());
        }
        self.coin_records(
            "get_coin_records_by_puzzle_hashes",
            json!({
                "puzzle_hashes": puzzle_hashes,
                "include_spent_coins": include_spent,
            }),
            start_height,
            end_height,
        )
        .await
    }

    /// By hints.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn by_hints(
        &self,
        hints: &[String],
        include_spent: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        if hints.is_empty() {
            return Ok(Vec::new());
        }
        self.coin_records(
            "get_coin_records_by_hints",
            json!({
                "hints": hints,
                "include_spent_coins": include_spent,
            }),
            start_height,
            end_height,
        )
        .await
    }

    /// By names.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn by_names(
        &self,
        coin_names: &[String],
        include_spent: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        if coin_names.is_empty() {
            return Ok(Vec::new());
        }
        self.coin_records(
            "get_coin_records_by_names",
            json!({
                "names": coin_names,
                "include_spent_coins": include_spent,
            }),
            start_height,
            end_height,
        )
        .await
    }

    /// Puzzle and solution.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn puzzle_and_solution(
        &self,
        coin_id_hex: &str,
        height: u64,
    ) -> SignerResult<Option<Value>> {
        let mut body = json!({ "coin_id": coin_id_hex });
        if height > 0 {
            body["height"] = json!(height);
        }
        self.record("get_puzzle_and_solution", body, "coin_solution")
            .await
    }

    /// Existing coin names.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn existing_coin_names(&self, coin_ids_hex: &[String]) -> SignerResult<Vec<String>> {
        let mut existing = Vec::new();
        for batch in chunk_values(coin_ids_hex, 200) {
            let names: Vec<String> = batch
                .iter()
                .filter_map(|coin_id| {
                    hex_to_bytes32(coin_id)
                        .ok()
                        .map(|bytes| to_coinset_hex(bytes.as_ref()))
                })
                .collect();
            let rows = self.by_names(&names, true, None, None).await?;
            for record in rows {
                let resolved = coin_id_from_record(&record);
                if !resolved.is_empty() {
                    existing.push(resolved);
                }
            }
        }
        Ok(existing)
    }
}

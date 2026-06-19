//! Script-facing Coinset read/mutation client for direct API hosts (`api.coinset.org`).
//!
//! Operator paths use [`super::super::MspCoinset`] against MSP; this client mirrors the
//! former Python `greenfloor.adapters.coinset` surface for vault scan scripts.

use serde_json::{json, Map, Value};

use super::network::{normalize_coinset_network, resolve_coinset_base_url};
use super::parse::{apply_height_range, coin_records_from_payload, record_from_payload};
use crate::coinset::{post_coinset_rpc, push_tx_hex};
use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone)]
pub(crate) struct CoinsetReadClient {
    pub network: String,
    pub base_url: String,
}

impl CoinsetReadClient {
    pub fn new(base_url: Option<&str>, network: &str) -> Self {
        let network = normalize_coinset_network(network).to_string();
        Self {
            network: network.clone(),
            base_url: resolve_coinset_base_url(&network, base_url),
        }
    }

    pub async fn post_json(&self, endpoint: &str, body: Value) -> SignerResult<Value> {
        let payload =
            post_coinset_rpc(&self.network, Some(self.base_url.as_str()), endpoint, body).await?;
        if !payload.is_object() {
            return Err(SignerError::Other(
                "coinset_invalid_response_payload".to_string(),
            ));
        }
        Ok(payload)
    }

    pub async fn push_tx(&self, spend_bundle_hex: &str) -> SignerResult<Value> {
        let payload = push_tx_hex(
            &self.network,
            Some(self.base_url.as_str()),
            spend_bundle_hex,
        )
        .await?;
        if !payload.is_object() {
            return Err(SignerError::Other(
                "coinset_push_tx_invalid_response".to_string(),
            ));
        }
        Ok(payload)
    }

    async fn records_from_post(
        &self,
        endpoint: &str,
        mut body: Map<String, Value>,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        apply_height_range(&mut body, start_height, end_height);
        let payload = self.post_json(endpoint, Value::Object(body)).await?;
        Ok(coin_records_from_payload(&payload))
    }

    async fn record_from_post(
        &self,
        endpoint: &str,
        body: Map<String, Value>,
        key: &str,
    ) -> SignerResult<Option<Value>> {
        let payload = self.post_json(endpoint, Value::Object(body)).await?;
        Ok(record_from_payload(&payload, key).cloned())
    }

    async fn coin_records_list_query(
        &self,
        endpoint: &str,
        list_field: &str,
        values_hex: &[String],
        include_spent_coins: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        if values_hex.is_empty() {
            return Ok(Vec::new());
        }
        let mut body = Map::new();
        body.insert(
            list_field.to_string(),
            json!(values_hex.iter().map(String::as_str).collect::<Vec<_>>()),
        );
        body.insert(
            "include_spent_coins".to_string(),
            json!(include_spent_coins),
        );
        self.records_from_post(endpoint, body, start_height, end_height)
            .await
    }

    pub async fn get_all_mempool_tx_ids(&self) -> SignerResult<Vec<String>> {
        let payload = self.post_json("get_all_mempool_tx_ids", json!({})).await?;
        if !payload
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(Vec::new());
        }
        let tx_ids = payload
            .get("tx_ids")
            .or_else(|| payload.get("mempool_tx_ids"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(tx_ids
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect())
    }

    pub async fn get_coin_records_by_puzzle_hash(
        &self,
        puzzle_hash_hex: &str,
        include_spent_coins: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        let mut body = Map::new();
        body.insert("puzzle_hash".to_string(), json!(puzzle_hash_hex));
        body.insert(
            "include_spent_coins".to_string(),
            json!(include_spent_coins),
        );
        self.records_from_post(
            "get_coin_records_by_puzzle_hash",
            body,
            start_height,
            end_height,
        )
        .await
    }

    pub async fn get_coin_records_by_puzzle_hashes(
        &self,
        puzzle_hashes_hex: &[String],
        include_spent_coins: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        self.coin_records_list_query(
            "get_coin_records_by_puzzle_hashes",
            "puzzle_hashes",
            puzzle_hashes_hex,
            include_spent_coins,
            start_height,
            end_height,
        )
        .await
    }

    pub async fn get_coin_record_by_name(
        &self,
        coin_name_hex: &str,
    ) -> SignerResult<Option<Value>> {
        let mut body = Map::new();
        body.insert("name".to_string(), json!(coin_name_hex));
        self.record_from_post("get_coin_record_by_name", body, "coin_record")
            .await
    }

    pub async fn get_coin_records_by_names(
        &self,
        coin_names_hex: &[String],
        include_spent_coins: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        self.coin_records_list_query(
            "get_coin_records_by_names",
            "names",
            coin_names_hex,
            include_spent_coins,
            start_height,
            end_height,
        )
        .await
    }

    pub async fn get_coin_records_by_parent_ids(
        &self,
        parent_ids_hex: &[String],
        include_spent_coins: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        self.coin_records_list_query(
            "get_coin_records_by_parent_ids",
            "parent_ids",
            parent_ids_hex,
            include_spent_coins,
            start_height,
            end_height,
        )
        .await
    }

    pub async fn get_coin_records_by_hint(
        &self,
        hint_hex: &str,
        include_spent_coins: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        let mut body = Map::new();
        body.insert("hint".to_string(), json!(hint_hex));
        body.insert(
            "include_spent_coins".to_string(),
            json!(include_spent_coins),
        );
        self.records_from_post("get_coin_records_by_hint", body, start_height, end_height)
            .await
    }

    pub async fn get_coin_records_by_hints(
        &self,
        hints_hex: &[String],
        include_spent_coins: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        self.coin_records_list_query(
            "get_coin_records_by_hints",
            "hints",
            hints_hex,
            include_spent_coins,
            start_height,
            end_height,
        )
        .await
    }

    pub async fn get_puzzle_and_solution(
        &self,
        coin_id_hex: &str,
        height: Option<u64>,
    ) -> SignerResult<Option<Value>> {
        let mut body = Map::new();
        body.insert("coin_id".to_string(), json!(coin_id_hex));
        if let Some(height) = height.filter(|value| *value > 0) {
            body.insert("height".to_string(), json!(height));
        }
        self.record_from_post("get_puzzle_and_solution", body, "coin_solution")
            .await
    }

    pub async fn get_blockchain_state(&self) -> SignerResult<Option<Value>> {
        let payload = self.post_json("get_blockchain_state", json!({})).await?;
        if !payload
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(None);
        }
        Ok(payload
            .get("blockchain_state")
            .filter(|value| value.is_object())
            .cloned())
    }
}

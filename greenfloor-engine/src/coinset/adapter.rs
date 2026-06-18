//! Script-facing Coinset read/mutation client for direct API hosts (`api.coinset.org`).
//!
//! Operator paths use [`super::MspCoinset`] against MSP; this adapter mirrors the former
//! Python `greenfloor.adapters.coinset` surface for vault scan scripts.

use serde_json::{json, Map, Value};

use crate::coinset::{post_coinset_rpc, push_tx_hex};
use crate::error::{SignerError, SignerResult};

pub const MAINNET_BASE_URL: &str = "https://api.coinset.org";
pub const TESTNET11_BASE_URL: &str = "https://testnet11.api.coinset.org";
const DEFAULT_WEBHOOK_PORT: &str = "8787";
const DEFAULT_WEBHOOK_PATH: &str = "/coinset/tx-block";

pub fn normalize_coinset_network(network: &str) -> &'static str {
    match network.trim().to_ascii_lowercase().as_str() {
        "testnet" | "testnet11" => "testnet11",
        _ => "mainnet",
    }
}

pub fn resolve_coinset_base_url(network: &str, base_url: Option<&str>) -> String {
    let trimmed = base_url.map_or("", str::trim).trim_end_matches('/');
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    if normalize_coinset_network(network) == "testnet11" {
        TESTNET11_BASE_URL.to_string()
    } else {
        MAINNET_BASE_URL.to_string()
    }
}

pub fn build_webhook_callback_url(listen_addr: &str, path: Option<&str>) -> String {
    let path = path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_WEBHOOK_PATH);
    let (host, port) = match listen_addr.split_once(':') {
        Some((host, port)) if !port.is_empty() => (host, port),
        _ => (listen_addr.trim(), DEFAULT_WEBHOOK_PORT),
    };
    format!("http://{host}:{port}{path}")
}

fn apply_height_range(
    body: &mut Map<String, Value>,
    start_height: Option<u64>,
    end_height: Option<u64>,
) {
    if let Some(start) = start_height {
        body.insert("start_height".to_string(), json!(start));
    }
    if let Some(end) = end_height {
        body.insert("end_height".to_string(), json!(end));
    }
}

fn coin_records_from_payload(payload: &Value) -> Vec<Value> {
    if !payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Vec::new();
    }
    payload
        .get("coin_records")
        .and_then(Value::as_array)
        .map(|records| {
            records
                .iter()
                .filter(|record| record.is_object())
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn record_from_payload<'a>(payload: &'a Value, key: &str) -> Option<&'a Value> {
    if !payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    payload.get(key).filter(|value| value.is_object())
}

#[derive(Debug, Clone)]
pub struct CoinsetReadClient {
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
        if let Some(state) = payload
            .get("blockchain_state")
            .filter(|value| value.is_object())
        {
            return Ok(Some(state.clone()));
        }
        Ok(Some(payload))
    }
}

#[derive(Debug, Clone)]
pub struct CoinsetAdapter {
    client: CoinsetReadClient,
}

impl CoinsetAdapter {
    pub fn new(base_url: Option<&str>, network: &str) -> Self {
        Self {
            client: CoinsetReadClient::new(base_url, network),
        }
    }

    pub fn network(&self) -> &str {
        &self.client.network
    }

    pub fn base_url(&self) -> &str {
        &self.client.base_url
    }

    pub fn read_client(&self) -> &CoinsetReadClient {
        &self.client
    }

    pub async fn post_json(&self, endpoint: &str, body: Value) -> SignerResult<Value> {
        self.client.post_json(endpoint, body).await
    }

    pub async fn get_all_mempool_tx_ids(&self) -> SignerResult<Vec<String>> {
        self.client.get_all_mempool_tx_ids().await
    }

    pub async fn get_coin_records_by_puzzle_hash(
        &self,
        puzzle_hash_hex: &str,
        include_spent_coins: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        self.client
            .get_coin_records_by_puzzle_hash(
                puzzle_hash_hex,
                include_spent_coins,
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
        self.client
            .get_coin_records_by_puzzle_hashes(
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
        self.client.get_coin_record_by_name(coin_name_hex).await
    }

    pub async fn get_coin_records_by_names(
        &self,
        coin_names_hex: &[String],
        include_spent_coins: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        self.client
            .get_coin_records_by_names(
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
        self.client
            .get_coin_records_by_parent_ids(
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
        self.client
            .get_coin_records_by_hint(hint_hex, include_spent_coins, start_height, end_height)
            .await
    }

    pub async fn get_coin_records_by_hints(
        &self,
        hints_hex: &[String],
        include_spent_coins: bool,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> SignerResult<Vec<Value>> {
        self.client
            .get_coin_records_by_hints(hints_hex, include_spent_coins, start_height, end_height)
            .await
    }

    pub async fn get_puzzle_and_solution(
        &self,
        coin_id_hex: &str,
        height: Option<u64>,
    ) -> SignerResult<Option<Value>> {
        self.client
            .get_puzzle_and_solution(coin_id_hex, height)
            .await
    }

    pub async fn get_blockchain_state(&self) -> SignerResult<Option<Value>> {
        self.client.get_blockchain_state().await
    }

    pub async fn push_tx(&self, spend_bundle_hex: &str) -> SignerResult<Value> {
        let payload = push_tx_hex(
            &self.client.network,
            Some(self.client.base_url.as_str()),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_coinset_network_maps_testnet_aliases() {
        assert_eq!(normalize_coinset_network("testnet"), "testnet11");
        assert_eq!(normalize_coinset_network("testnet11"), "testnet11");
        assert_eq!(normalize_coinset_network("mainnet"), "mainnet");
        assert_eq!(normalize_coinset_network("unknown"), "mainnet");
    }

    #[test]
    fn resolve_coinset_base_url_defaults_by_network() {
        assert_eq!(
            resolve_coinset_base_url("mainnet", None),
            MAINNET_BASE_URL.to_string()
        );
        assert_eq!(
            resolve_coinset_base_url("testnet11", None),
            TESTNET11_BASE_URL.to_string()
        );
        assert_eq!(
            resolve_coinset_base_url("testnet11", Some("https://coinset.custom")),
            "https://coinset.custom".to_string()
        );
    }

    #[test]
    fn build_webhook_callback_url_defaults() {
        assert_eq!(
            build_webhook_callback_url("127.0.0.1:8787", None),
            "http://127.0.0.1:8787/coinset/tx-block"
        );
        assert_eq!(
            build_webhook_callback_url("0.0.0.0", None),
            "http://0.0.0.0:8787/coinset/tx-block"
        );
        assert_eq!(
            build_webhook_callback_url("localhost:9090", Some("/custom")),
            "http://localhost:9090/custom"
        );
    }

    #[test]
    fn coin_records_from_payload_filters_non_objects() {
        let payload = json!({
            "success": true,
            "coin_records": [{"coin": {"amount": 1}}, "bad"]
        });
        let records = coin_records_from_payload(&payload);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["coin"]["amount"], 1);
    }

    #[test]
    fn coin_records_from_payload_returns_empty_on_failure() {
        let payload = json!({"success": false});
        assert!(coin_records_from_payload(&payload).is_empty());
    }

    #[test]
    fn record_from_payload_returns_none_on_failure() {
        let payload = json!({"success": false, "coin_record": {"coin": {"amount": 1}}});
        assert!(record_from_payload(&payload, "coin_record").is_none());
    }

    #[test]
    fn adapter_defaults_to_mainnet_base_url() {
        let adapter = CoinsetAdapter::new(None, "mainnet");
        assert_eq!(adapter.base_url(), MAINNET_BASE_URL);
        assert_eq!(adapter.network(), "mainnet");
    }

    #[test]
    fn adapter_network_testnet11() {
        let adapter = CoinsetAdapter::new(None, "testnet11");
        assert_eq!(adapter.base_url(), TESTNET11_BASE_URL);
        assert_eq!(adapter.network(), "testnet11");
    }

    #[tokio::test]
    async fn adapter_get_all_mempool_tx_ids_uses_post() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_all_mempool_tx_ids")
            .with_status(200)
            .with_body(r#"{"success":true,"mempool_tx_ids":["0xabc","0xdef"]}"#)
            .create_async()
            .await;

        let adapter = CoinsetAdapter::new(Some(&server.url()), "mainnet");
        let tx_ids = adapter
            .get_all_mempool_tx_ids()
            .await
            .expect("mempool tx ids");
        assert_eq!(tx_ids, vec!["0xabc".to_string(), "0xdef".to_string()]);
    }

    #[tokio::test]
    async fn adapter_get_coin_records_by_puzzle_hash_filters_non_dicts() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(r#"{"success":true,"coin_records":[{"coin":{"amount":1}},"bad"]}"#)
            .create_async()
            .await;

        let adapter = CoinsetAdapter::new(Some(&server.url()), "mainnet");
        let records = adapter
            .get_coin_records_by_puzzle_hash("0x11", false, None, None)
            .await
            .expect("coin records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["coin"]["amount"], 1);
    }

    #[tokio::test]
    async fn adapter_get_coin_record_by_name_success() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_record_by_name")
            .with_status(200)
            .with_body(r#"{"success":true,"coin_record":{"coin":{"amount":123}}}"#)
            .create_async()
            .await;

        let adapter = CoinsetAdapter::new(Some(&server.url()), "mainnet");
        let found = adapter
            .get_coin_record_by_name("0x22")
            .await
            .expect("coin record")
            .expect("some record");
        assert_eq!(found["coin"]["amount"], 123);
    }

    #[tokio::test]
    async fn adapter_get_coin_record_by_name_returns_none_on_failure() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_record_by_name")
            .with_status(200)
            .with_body(r#"{"success":false,"error":"not_found"}"#)
            .create_async()
            .await;

        let adapter = CoinsetAdapter::new(Some(&server.url()), "mainnet");
        assert!(adapter
            .get_coin_record_by_name("0x33")
            .await
            .expect("missing record")
            .is_none());
    }

    #[tokio::test]
    async fn adapter_get_puzzle_and_solution_adds_height_when_provided() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_puzzle_and_solution")
            .match_body(mockito::Matcher::Json(json!({
                "coin_id": "0x44",
                "height": 50
            })))
            .with_status(200)
            .with_body(r#"{"success":true,"coin_solution":{"puzzle_reveal":"80","solution":"80"}}"#)
            .create_async()
            .await;

        let adapter = CoinsetAdapter::new(Some(&server.url()), "mainnet");
        let solution = adapter
            .get_puzzle_and_solution("0x44", Some(50))
            .await
            .expect("puzzle and solution")
            .expect("some solution");
        assert_eq!(solution["puzzle_reveal"], "80");
        assert_eq!(solution["solution"], "80");
    }

    #[tokio::test]
    async fn adapter_get_puzzle_and_solution_omits_non_positive_height() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_puzzle_and_solution")
            .match_body(mockito::Matcher::Json(json!({"coin_id": "0x55"})))
            .with_status(200)
            .with_body(r#"{"success":true,"coin_solution":{"puzzle_reveal":"80","solution":"80"}}"#)
            .create_async()
            .await;

        let adapter = CoinsetAdapter::new(Some(&server.url()), "mainnet");
        let solution = adapter
            .get_puzzle_and_solution("0x55", Some(0))
            .await
            .expect("puzzle and solution")
            .expect("some solution");
        assert_eq!(solution["puzzle_reveal"], "80");
    }

    #[tokio::test]
    async fn adapter_get_blockchain_state_success() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_blockchain_state")
            .with_status(200)
            .with_body(r#"{"success":true,"blockchain_state":{"peak_height":1234}}"#)
            .create_async()
            .await;

        let adapter = CoinsetAdapter::new(Some(&server.url()), "mainnet");
        let state = adapter
            .get_blockchain_state()
            .await
            .expect("blockchain state")
            .expect("some state");
        assert_eq!(state["peak_height"], 1234);
    }

    #[tokio::test]
    async fn adapter_get_blockchain_state_returns_none_on_failure() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_blockchain_state")
            .with_status(200)
            .with_body(r#"{"success":false}"#)
            .create_async()
            .await;

        let adapter = CoinsetAdapter::new(Some(&server.url()), "mainnet");
        assert!(adapter
            .get_blockchain_state()
            .await
            .expect("failed state")
            .is_none());
    }

    #[tokio::test]
    async fn adapter_push_tx_returns_payload_dict() {
        use chia_protocol::SpendBundle;
        use chia_traits::Streamable;

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/push_tx")
            .with_status(200)
            .with_body(r#"{"success":true,"status":"SUCCESS"}"#)
            .create_async()
            .await;

        let bundle = SpendBundle::new(Vec::new(), chia_bls::Signature::default());
        let spend_bundle_hex = hex::encode(
            bundle
                .to_bytes()
                .expect("serialize empty spend bundle for adapter push tx test"),
        );

        let adapter = CoinsetAdapter::new(Some(&server.url()), "mainnet");
        let result = adapter.push_tx(&spend_bundle_hex).await.expect("push tx");
        assert_eq!(result["success"], true);
        assert_eq!(result["status"], "SUCCESS");
    }
}

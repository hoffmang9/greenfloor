use chia_protocol::SpendBundle;
use chia_traits::Streamable;
use serde_json::json;

use super::client::CoinsetReadClient;
use super::network::{MAINNET_BASE_URL, TESTNET11_BASE_URL};
use super::parse::{coin_records_from_payload, record_from_payload};
use super::{normalize_coinset_network, resolve_coinset_base_url};

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
fn client_defaults_to_mainnet_base_url() {
    let client = CoinsetReadClient::new(None, "mainnet");
    assert_eq!(client.base_url, MAINNET_BASE_URL);
    assert_eq!(client.network, "mainnet");
}

#[test]
fn client_network_testnet11() {
    let client = CoinsetReadClient::new(None, "testnet11");
    assert_eq!(client.base_url, TESTNET11_BASE_URL);
    assert_eq!(client.network, "testnet11");
}

#[tokio::test]
async fn client_get_all_mempool_tx_ids_uses_post() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_all_mempool_tx_ids")
        .with_status(200)
        .with_body(r#"{"success":true,"mempool_tx_ids":["0xabc","0xdef"]}"#)
        .create_async()
        .await;

    let client = CoinsetReadClient::new(Some(&server.url()), "mainnet");
    let tx_ids = client
        .get_all_mempool_tx_ids()
        .await
        .expect("mempool tx ids");
    assert_eq!(tx_ids, vec!["0xabc".to_string(), "0xdef".to_string()]);
}

#[tokio::test]
async fn client_get_coin_records_by_puzzle_hash_filters_non_dicts() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[{"coin":{"amount":1}},"bad"]}"#)
        .create_async()
        .await;

    let client = CoinsetReadClient::new(Some(&server.url()), "mainnet");
    let records = client
        .get_coin_records_by_puzzle_hash("0x11", false, None, None)
        .await
        .expect("coin records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["coin"]["amount"], 1);
}

#[tokio::test]
async fn client_get_coin_record_by_name_success() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_record_by_name")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_record":{"coin":{"amount":123}}}"#)
        .create_async()
        .await;

    let client = CoinsetReadClient::new(Some(&server.url()), "mainnet");
    let found = client
        .get_coin_record_by_name("0x22")
        .await
        .expect("coin record")
        .expect("some record");
    assert_eq!(found["coin"]["amount"], 123);
}

#[tokio::test]
async fn client_get_coin_record_by_name_returns_none_on_failure() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_record_by_name")
        .with_status(200)
        .with_body(r#"{"success":false,"error":"not_found"}"#)
        .create_async()
        .await;

    let client = CoinsetReadClient::new(Some(&server.url()), "mainnet");
    assert!(client
        .get_coin_record_by_name("0x33")
        .await
        .expect("missing record")
        .is_none());
}

#[tokio::test]
async fn client_get_puzzle_and_solution_adds_height_when_provided() {
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

    let client = CoinsetReadClient::new(Some(&server.url()), "mainnet");
    let solution = client
        .get_puzzle_and_solution("0x44", Some(50))
        .await
        .expect("puzzle and solution")
        .expect("some solution");
    assert_eq!(solution["puzzle_reveal"], "80");
    assert_eq!(solution["solution"], "80");
}

#[tokio::test]
async fn client_get_puzzle_and_solution_omits_non_positive_height() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_puzzle_and_solution")
        .match_body(mockito::Matcher::Json(json!({"coin_id": "0x55"})))
        .with_status(200)
        .with_body(r#"{"success":true,"coin_solution":{"puzzle_reveal":"80","solution":"80"}}"#)
        .create_async()
        .await;

    let client = CoinsetReadClient::new(Some(&server.url()), "mainnet");
    let solution = client
        .get_puzzle_and_solution("0x55", Some(0))
        .await
        .expect("puzzle and solution")
        .expect("some solution");
    assert_eq!(solution["puzzle_reveal"], "80");
}

#[tokio::test]
async fn client_get_blockchain_state_success() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_blockchain_state")
        .with_status(200)
        .with_body(r#"{"success":true,"blockchain_state":{"peak_height":1234}}"#)
        .create_async()
        .await;

    let client = CoinsetReadClient::new(Some(&server.url()), "mainnet");
    let state = client
        .get_blockchain_state()
        .await
        .expect("blockchain state")
        .expect("some state");
    assert_eq!(state["peak_height"], 1234);
}

#[tokio::test]
async fn client_get_blockchain_state_returns_none_on_failure() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_blockchain_state")
        .with_status(200)
        .with_body(r#"{"success":false}"#)
        .create_async()
        .await;

    let client = CoinsetReadClient::new(Some(&server.url()), "mainnet");
    assert!(client
        .get_blockchain_state()
        .await
        .expect("failed state")
        .is_none());
}

#[tokio::test]
async fn client_get_blockchain_state_returns_none_when_state_key_missing() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_blockchain_state")
        .with_status(200)
        .with_body(r#"{"success":true,"unexpected":"shape"}"#)
        .create_async()
        .await;

    let client = CoinsetReadClient::new(Some(&server.url()), "mainnet");
    assert!(client
        .get_blockchain_state()
        .await
        .expect("missing state key")
        .is_none());
}

#[tokio::test]
async fn client_push_tx_returns_payload_dict() {
    let bundle = SpendBundle::new(Vec::new(), chia_bls::Signature::default());
    let spend_bundle_hex = hex::encode(
        bundle
            .to_bytes()
            .expect("serialize empty spend bundle for client push tx test"),
    );

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/push_tx")
        .with_status(200)
        .with_body(r#"{"success":true,"status":"SUCCESS"}"#)
        .create_async()
        .await;

    let client = CoinsetReadClient::new(Some(&server.url()), "mainnet");
    let result = client.push_tx(&spend_bundle_hex).await.expect("push tx");
    assert_eq!(result["success"], true);
    assert_eq!(result["status"], "SUCCESS");
}

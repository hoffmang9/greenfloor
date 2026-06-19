use chia_protocol::SpendBundle;
use chia_traits::Streamable;
use serde_json::json;

use super::network::{
    normalize_coinset_network, resolve_coinset_base_url, MAINNET_BASE_URL, TESTNET11_BASE_URL,
};
use super::parse::{coin_records_from_payload, record_from_payload};
use crate::coinset::{post_coinset_rpc, push_tx_hex};

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

#[tokio::test]
async fn post_coinset_rpc_get_all_mempool_tx_ids() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_all_mempool_tx_ids")
        .with_status(200)
        .with_body(r#"{"success":true,"mempool_tx_ids":["0xabc","0xdef"]}"#)
        .create_async()
        .await;

    let payload = post_coinset_rpc(
        "mainnet",
        Some(&server.url()),
        "get_all_mempool_tx_ids",
        json!({}),
    )
    .await
    .expect("mempool tx ids");
    assert_eq!(
        payload
            .get("mempool_tx_ids")
            .and_then(|value| value.as_array())
            .map(|values| values.len()),
        Some(2)
    );
}

#[tokio::test]
async fn post_coinset_rpc_coin_records_by_puzzle_hash_filters_via_parse() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[{"coin":{"amount":1}},"bad"]}"#)
        .create_async()
        .await;

    let payload = post_coinset_rpc(
        "mainnet",
        Some(&server.url()),
        "get_coin_records_by_puzzle_hash",
        json!({"puzzle_hash": "0x11", "include_spent_coins": false}),
    )
    .await
    .expect("coin records");
    let records = coin_records_from_payload(&payload);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["coin"]["amount"], 1);
}

#[tokio::test]
async fn post_coinset_rpc_get_coin_record_by_name() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_record_by_name")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_record":{"coin":{"amount":123}}}"#)
        .create_async()
        .await;

    let payload = post_coinset_rpc(
        "mainnet",
        Some(&server.url()),
        "get_coin_record_by_name",
        json!({"name": "0x22"}),
    )
    .await
    .expect("coin record");
    let found = record_from_payload(&payload, "coin_record").expect("some record");
    assert_eq!(found["coin"]["amount"], 123);
}

#[tokio::test]
async fn post_coinset_rpc_get_blockchain_state() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_blockchain_state")
        .with_status(200)
        .with_body(r#"{"success":true,"blockchain_state":{"peak_height":1234}}"#)
        .create_async()
        .await;

    let payload = post_coinset_rpc(
        "mainnet",
        Some(&server.url()),
        "get_blockchain_state",
        json!({}),
    )
    .await
    .expect("blockchain state");
    let state = record_from_payload(&payload, "blockchain_state").expect("some state");
    assert_eq!(state["peak_height"], 1234);
}

#[tokio::test]
async fn push_tx_hex_returns_success_payload() {
    let bundle = SpendBundle::new(Vec::new(), chia_bls::Signature::default());
    let spend_bundle_hex = hex::encode(
        bundle
            .to_bytes()
            .expect("serialize empty spend bundle for push tx test"),
    );

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/push_tx")
        .with_status(200)
        .with_body(r#"{"success":true,"status":"SUCCESS"}"#)
        .create_async()
        .await;

    let result = push_tx_hex("mainnet", Some(&server.url()), &spend_bundle_hex)
        .await
        .expect("push tx");
    assert_eq!(
        result.get("success").and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        result.get("status").and_then(|value| value.as_str()),
        Some("SUCCESS")
    );
}

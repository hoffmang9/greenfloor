use chia_protocol::SpendBundle;
use chia_traits::Streamable;
use serde_json::{json, Value};

use super::{
    conservative_fee_from_payload, get_all_mempool_tx_ids, get_fee_estimate,
    post_coinset_coin_records, post_coinset_record, post_coinset_rpc, push_tx_hex,
};

#[test]
fn conservative_fee_uses_max_estimate() {
    let payload = json!({"success": true, "estimates": [100, 500, 200]});
    assert_eq!(conservative_fee_from_payload(&payload), Some(500));
}

#[test]
fn conservative_fee_falls_back_to_fee_estimate_field() {
    let payload = json!({"success": true, "fee_estimate": 42});
    assert_eq!(conservative_fee_from_payload(&payload), Some(42));
}

#[test]
fn conservative_fee_returns_none_on_failure() {
    let payload = json!({"success": false});
    assert_eq!(conservative_fee_from_payload(&payload), None);
}

#[tokio::test]
async fn get_fee_estimate_via_direct_coinset_client() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_fee_estimate")
        .with_status(200)
        .with_body(r#"{"success":true,"estimates":[100,500]}"#)
        .create_async()
        .await;

    let payload = get_fee_estimate("mainnet", Some(&server.url()), vec![300], 1_000_000, None)
        .await
        .expect("fee estimate");
    assert_eq!(conservative_fee_from_payload(&payload), Some(500));
}

#[tokio::test]
async fn get_all_mempool_tx_ids_via_direct_coinset_client() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_all_mempool_tx_ids")
        .with_status(200)
        .with_body(r#"{"success":true,"tx_ids":["0xabc"]}"#)
        .create_async()
        .await;

    let tx_ids = get_all_mempool_tx_ids("mainnet", Some(&server.url()))
        .await
        .expect("mempool tx ids");
    assert_eq!(tx_ids, vec!["0xabc".to_string()]);
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
            .map(std::vec::Vec::len),
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

    let records = post_coinset_coin_records(
        "mainnet",
        Some(&server.url()),
        "get_coin_records_by_puzzle_hash",
        json!({"puzzle_hash": "0x11", "include_spent_coins": false}),
    )
    .await
    .expect("coin records");
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

    let found = post_coinset_record(
        "mainnet",
        Some(&server.url()),
        "get_coin_record_by_name",
        json!({"name": "0x22"}),
        "coin_record",
    )
    .await
    .expect("coin record")
    .expect("some record");
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

    let state = post_coinset_record(
        "mainnet",
        Some(&server.url()),
        "get_blockchain_state",
        json!({}),
        "blockchain_state",
    )
    .await
    .expect("blockchain state")
    .expect("some state");
    assert_eq!(state["peak_height"], 1234);
}

#[tokio::test]
async fn post_coinset_rpc_accepts_testnet_alias() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_blockchain_state")
        .with_status(200)
        .with_body(r#"{"success":true,"blockchain_state":{"peak_height":1}}"#)
        .create_async()
        .await;

    let state = post_coinset_record(
        "testnet",
        Some(&server.url()),
        "get_blockchain_state",
        json!({}),
        "blockchain_state",
    )
    .await
    .expect("testnet alias")
    .expect("some state");
    assert_eq!(state["peak_height"], 1);
}

#[tokio::test]
async fn post_coinset_coin_records_fails_on_success_false() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(r#"{"success":false,"error":"invalid puzzle hash"}"#)
        .create_async()
        .await;

    let err = post_coinset_coin_records(
        "mainnet",
        Some(&server.url()),
        "get_coin_records_by_puzzle_hash",
        json!({"puzzle_hash": "0x11", "include_spent_coins": false}),
    )
    .await
    .expect_err("success=false should fail");
    assert_eq!(err.to_string(), "coinset error: invalid puzzle hash");
}

#[tokio::test]
async fn post_coinset_rpc_surfaces_http_503_as_coinset_error() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_blockchain_state")
        .with_status(503)
        .with_body("service unavailable")
        .create_async()
        .await;

    let err = post_coinset_rpc(
        "mainnet",
        Some(&server.url()),
        "get_blockchain_state",
        json!({}),
    )
    .await
    .expect_err("503 should fail");
    let message = err.to_string();
    assert!(message.starts_with("coinset error:"), "{message}");
    assert_eq!(
        message, "coinset error: error decoding response body",
        "unexpected coinset 503 error text"
    );
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
    assert_eq!(result.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(
        result.get("status").and_then(|value| value.as_str()),
        Some("SUCCESS")
    );
}

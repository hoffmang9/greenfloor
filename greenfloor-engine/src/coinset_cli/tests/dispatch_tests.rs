use crate::cli_util::script_engine_error_retryable;
use crate::coinset::{
    coin_id_from_record, ensure_coinset_rpc_success, post_coinset_coin_records,
    post_coinset_record, post_coinset_rpc, push_tx_hex, resolve_direct_client,
    TESTNET11_DIRECT_BASE_URL,
};
use crate::coinset_cli::{
    run_coinset_command, CoinsetCliArgs, CoinsetClientArgs, CoinsetCoinIdFromRecordArgs,
    CoinsetCommands, CoinsetPostArgs, CoinsetResolveClientArgs,
};
use crate::error::SignerError;
use chia_protocol::SpendBundle;
use chia_protocol::{Bytes32, Coin};
use chia_traits::Streamable;
use mockito::Matcher;
use serde_json::json;

#[test]
fn resolve_client_testnet_defaults_without_base_url() {
    let resolved = resolve_direct_client("testnet", None);
    assert_eq!(resolved.network, "testnet11");
    assert_eq!(resolved.base_url, TESTNET11_DIRECT_BASE_URL);
}

#[tokio::test]
async fn coin_records_testnet_without_base_url() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_blockchain_state")
        .with_status(200)
        .with_body(r#"{"success":true,"blockchain_state":{"peak_height":42}}"#)
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
    .expect("record")
    .expect("some state");
    assert_eq!(
        state.get("peak_height").and_then(serde_json::Value::as_i64),
        Some(42)
    );
}

#[tokio::test]
async fn coin_records_filters_non_objects_and_height_flags() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .match_body(Matcher::PartialJson(json!({
            "puzzle_hash": "0x11",
            "include_spent_coins": false,
            "start_height": 10,
            "end_height": 20,
        })))
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[{"coin":{"amount":1}},"bad"]}"#)
        .create_async()
        .await;

    let records = post_coinset_coin_records(
        "mainnet",
        Some(&server.url()),
        "get_coin_records_by_puzzle_hash",
        json!({
            "puzzle_hash": "0x11",
            "include_spent_coins": false,
            "start_height": 10,
            "end_height": 20,
        }),
    )
    .await
    .expect("coin records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["coin"]["amount"], 1);
}

#[tokio::test]
async fn coin_records_fails_on_success_false() {
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
        json!({"puzzle_hash":"0x11","include_spent_coins":false}),
    )
    .await
    .expect_err("success=false");
    assert_eq!(err.to_string(), "coinset error: invalid puzzle hash");
    assert!(!script_engine_error_retryable(&err));
}

#[tokio::test]
async fn coin_records_surfaces_retryable_error_on_http_503() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(503)
        .with_body("service unavailable")
        .expect_at_least(1)
        .create_async()
        .await;

    let err = post_coinset_coin_records(
        "mainnet",
        Some(&server.url()),
        "get_coin_records_by_puzzle_hash",
        json!({"puzzle_hash":"0x11","include_spent_coins":false}),
    )
    .await
    .expect_err("503");
    assert!(matches!(err, SignerError::Coinset(_)));
    assert!(script_engine_error_retryable(&err));
}

#[tokio::test]
async fn post_fee_estimate_returns_rpc_payload() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_fee_estimate")
        .with_status(200)
        .with_body(r#"{"success":true,"estimates":[100,500,200]}"#)
        .create_async()
        .await;

    let value = post_coinset_rpc(
        "mainnet",
        Some(&server.url()),
        "get_fee_estimate",
        json!({"target_times":[300,600,1200],"cost":1_000_000}),
    )
    .await
    .expect("fee estimate");
    assert_eq!(
        value.get("success").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    let estimates = value
        .get("estimates")
        .and_then(serde_json::Value::as_array)
        .expect("estimates");
    assert_eq!(estimates.len(), 3);
}

#[tokio::test]
async fn post_returns_rpc_payload() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_all_mempool_tx_ids")
        .with_status(200)
        .with_body(r#"{"success":true,"mempool_tx_ids":["0xabc","0xdef"]}"#)
        .create_async()
        .await;

    let value = post_coinset_rpc(
        "mainnet",
        Some(&server.url()),
        "get_all_mempool_tx_ids",
        json!({}),
    )
    .await
    .expect("post");
    let tx_ids = value
        .get("mempool_tx_ids")
        .and_then(serde_json::Value::as_array)
        .expect("mempool_tx_ids");
    assert_eq!(tx_ids.len(), 2);
}

#[tokio::test]
async fn post_fails_on_success_false() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_all_mempool_tx_ids")
        .with_status(200)
        .with_body(r#"{"success":false,"error":"mempool unavailable"}"#)
        .create_async()
        .await;

    let payload = post_coinset_rpc(
        "mainnet",
        Some(&server.url()),
        "get_all_mempool_tx_ids",
        json!({}),
    )
    .await
    .expect("rpc payload");
    let err = ensure_coinset_rpc_success(&payload).expect_err("success=false");
    assert_eq!(err.to_string(), "coinset error: mempool unavailable");
}

#[test]
fn coin_id_from_record_computes_coin_id() {
    let parent = Bytes32::new([0x11; 32]);
    let puzzle_hash = Bytes32::new([0x22; 32]);
    let amount = 42_u64;
    let expected = hex::encode(Coin::new(parent, puzzle_hash, amount).coin_id());
    let record = json!({
        "coin": {
            "parent_coin_info": format!("0x{}", hex::encode(parent)),
            "puzzle_hash": format!("0x{}", hex::encode(puzzle_hash)),
            "amount": amount,
        }
    });
    assert_eq!(coin_id_from_record(&record), expected);
}

#[tokio::test]
async fn push_tx_emits_success_payload() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/push_tx")
        .with_status(200)
        .with_body(r#"{"success":true,"status":"SUCCESS"}"#)
        .create_async()
        .await;

    let bundle = SpendBundle::new(Vec::new(), chia_bls::Signature::default());
    let spend_bundle_hex = hex::encode(bundle.to_bytes().expect("serialize bundle"));
    let value = push_tx_hex("mainnet", Some(&server.url()), &spend_bundle_hex)
        .await
        .expect("push tx");
    assert_eq!(
        value.get("success").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value.get("status").and_then(serde_json::Value::as_str),
        Some("SUCCESS")
    );
}

#[tokio::test]
async fn run_coinset_command_dispatches_subcommands() {
    let resolved = resolve_direct_client("mainnet", None);
    let resolve_args = CoinsetCliArgs {
        command: CoinsetCommands::ResolveClient(CoinsetResolveClientArgs {
            client: CoinsetClientArgs {
                network: "mainnet".to_string(),
                base_url: String::new(),
            },
            json: true,
        }),
    };
    run_coinset_command(resolve_args)
        .await
        .expect("resolve-client");
    assert_eq!(resolved.network, "mainnet");

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/get_all_mempool_tx_ids")
        .with_status(200)
        .with_body(r#"{"success":true,"mempool_tx_ids":[]}"#)
        .create_async()
        .await;
    let post_args = CoinsetCliArgs {
        command: CoinsetCommands::Post(CoinsetPostArgs {
            client: CoinsetClientArgs {
                network: "mainnet".to_string(),
                base_url: server.url(),
            },
            endpoint: "get_all_mempool_tx_ids".to_string(),
            body_json: "{}".to_string(),
            json: true,
        }),
    };
    run_coinset_command(post_args).await.expect("post");
    mock.assert();

    let parent = Bytes32::new([0x11; 32]);
    let puzzle_hash = Bytes32::new([0x22; 32]);
    let amount = 42_u64;
    let expected_coin_id = hex::encode(Coin::new(parent, puzzle_hash, amount).coin_id());
    let record = json!({
        "coin": {
            "parent_coin_info": format!("0x{}", hex::encode(parent)),
            "puzzle_hash": format!("0x{}", hex::encode(puzzle_hash)),
            "amount": amount,
        }
    });
    assert_eq!(coin_id_from_record(&record), expected_coin_id);
    let coin_id_args = CoinsetCliArgs {
        command: CoinsetCommands::CoinIdFromRecord(CoinsetCoinIdFromRecordArgs {
            record_json: record.to_string(),
            json: true,
        }),
    };
    run_coinset_command(coin_id_args)
        .await
        .expect("coin-id-from-record");
}

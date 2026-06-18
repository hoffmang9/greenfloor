use chia_protocol::SpendBundle;
use chia_traits::Streamable;
use serde_json::Value;

#[tokio::test]
async fn subprocess_coinset_conservative_fee_emits_json_envelope() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_fee_estimate")
        .with_status(200)
        .with_body(r#"{"success":true,"estimates":[100,500,200]}"#)
        .create_async()
        .await;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "coinset",
            "conservative-fee-estimate",
            "--network",
            "mainnet",
            "--base-url",
            &server.url(),
            "--cost",
            "1000000",
            "--json",
        ])
        .output()
        .expect("spawn greenfloor-engine coinset subprocess");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value =
        serde_json::from_slice(&output.stdout).expect("parse conservative fee json stdout");
    assert_eq!(value.get("fee_mojos").and_then(Value::as_u64), Some(500));
}

#[tokio::test]
async fn subprocess_coinset_post_returns_rpc_payload() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_all_mempool_tx_ids")
        .with_status(200)
        .with_body(r#"{"success":true,"mempool_tx_ids":["0xabc","0xdef"]}"#)
        .create_async()
        .await;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "coinset",
            "post",
            "--network",
            "mainnet",
            "--base-url",
            &server.url(),
            "--endpoint",
            "get_all_mempool_tx_ids",
            "--body-json",
            "{}",
            "--json",
        ])
        .output()
        .expect("spawn greenfloor-engine coinset post subprocess");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("parse post json stdout");
    assert_eq!(value.get("success").and_then(Value::as_bool), Some(true));
    let tx_ids = value
        .get("mempool_tx_ids")
        .and_then(Value::as_array)
        .expect("mempool_tx_ids array");
    assert_eq!(tx_ids.len(), 2);
}

#[tokio::test]
async fn subprocess_coinset_push_tx_emits_success_payload() {
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
            .expect("serialize empty spend bundle for subprocess smoke test"),
    );

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "coinset",
            "push-tx",
            "--network",
            "mainnet",
            "--base-url",
            &server.url(),
            "--spend-bundle-hex",
            &spend_bundle_hex,
            "--json",
        ])
        .output()
        .expect("spawn greenfloor-engine coinset push-tx subprocess");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value =
        serde_json::from_slice(&output.stdout).expect("parse push-tx json stdout");
    assert_eq!(value.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(value.get("status").and_then(Value::as_str), Some("SUCCESS"));
}

use mockito::Matcher;
use serde_json::Value;

#[path = "fixtures/json_util.rs"]
mod json_util;

use json_util::parse_json_output;

#[tokio::test]
async fn subprocess_coinset_record_returns_parsed_payload() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_blockchain_state")
        .match_body(Matcher::Json(serde_json::json!({})))
        .with_status(200)
        .with_body(r#"{"success":true,"blockchain_state":{"peak_height":99}}"#)
        .create_async()
        .await;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "coinset",
            "record",
            "--network",
            "mainnet",
            "--base-url",
            &server.url(),
            "--endpoint",
            "get_blockchain_state",
            "--body-json",
            "{}",
            "--key",
            "blockchain_state",
            "--json",
        ])
        .output()
        .expect("spawn coinset record subprocess");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = parse_json_output(&output.stdout);
    assert_eq!(
        payload
            .get("record")
            .and_then(|record| record.get("peak_height"))
            .and_then(Value::as_i64),
        Some(99)
    );
}

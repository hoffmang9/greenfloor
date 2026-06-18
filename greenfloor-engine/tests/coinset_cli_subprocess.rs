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

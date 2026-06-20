//! Single subprocess smoke for `greenfloor-engine coinset` CLI wiring.
//!
//! HTTP behavior and retry classification are covered in `coinset_cli` and `coinset::api` unit tests.

use greenfloor_engine::coinset::TESTNET11_DIRECT_BASE_URL;
use serde_json::Value;

#[test]
fn subprocess_coinset_cli_smoke() {
    let bin = env!("CARGO_BIN_EXE_greenfloor-engine");

    let resolve = std::process::Command::new(bin)
        .args([
            "coinset",
            "resolve-client",
            "--network",
            "testnet",
            "--json",
        ])
        .output()
        .expect("spawn greenfloor-engine coinset resolve-client subprocess");
    assert!(
        resolve.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&resolve.stderr)
    );
    let value: Value =
        serde_json::from_slice(&resolve.stdout).expect("parse resolve-client json stdout");
    assert_eq!(
        value.get("network").and_then(Value::as_str),
        Some("testnet11")
    );
    assert_eq!(
        value.get("base_url").and_then(Value::as_str),
        Some(TESTNET11_DIRECT_BASE_URL)
    );

    let coin_records = std::process::Command::new(bin)
        .args([
            "coinset",
            "coin-records",
            "--network",
            "mainnet",
            "--base-url",
            "http://127.0.0.1:1",
            "--endpoint",
            "get_coin_records_by_puzzle_hash",
            "--body-json",
            r#"{"puzzle_hash":"0x11","include_spent_coins":false}"#,
            "--json",
        ])
        .output()
        .expect("spawn greenfloor-engine coinset coin-records subprocess");
    assert!(
        !coin_records.status.success(),
        "expected non-zero exit for connection refused, stdout: {}",
        String::from_utf8_lossy(&coin_records.stdout)
    );
    let stderr = String::from_utf8_lossy(&coin_records.stderr);
    let payload: Value = serde_json::from_str(stderr.trim()).expect("parse json error stderr");
    assert_eq!(payload.get("success"), Some(&Value::Bool(false)));
    assert_eq!(payload.get("retryable"), Some(&Value::Bool(true)));
}

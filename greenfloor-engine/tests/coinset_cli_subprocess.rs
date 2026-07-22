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
}

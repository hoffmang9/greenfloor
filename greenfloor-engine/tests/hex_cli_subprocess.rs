use serde_json::Value;

#[test]
fn subprocess_hex_normalize_batch_returns_normalized_values() {
    let valid_id = "a".repeat(64);
    let values_json = format!(r#"["0x{valid_id}","not-hex"]"#);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "hex",
            "normalize-batch",
            "--values-json",
            &values_json,
            "--json",
        ])
        .output()
        .expect("spawn greenfloor-engine hex normalize-batch subprocess");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value =
        serde_json::from_slice(&output.stdout).expect("parse hex normalize-batch json stdout");
    let normalized = value
        .get("normalized")
        .and_then(Value::as_array)
        .expect("normalized array");
    assert_eq!(normalized.len(), 2);
    assert_eq!(normalized[0].as_str(), Some(valid_id.as_str()));
    assert_eq!(normalized[1].as_str(), Some(""));
}

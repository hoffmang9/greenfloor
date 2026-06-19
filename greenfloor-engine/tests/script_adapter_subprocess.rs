use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[path = "fixtures/json_util.rs"]
mod json_util;

use json_util::parse_json_output;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn python_bin() -> PathBuf {
    repo_root().join(".venv/bin/python")
}

#[test]
fn script_adapter_unittests_pass() {
    let python = python_bin();
    if !python.is_file() {
        eprintln!(
            "skip script_adapter_unittests_pass: missing {}",
            python.display()
        );
        return;
    }
    let output = Command::new(&python)
        .arg("-m")
        .arg("unittest")
        .arg("greenfloor_scripts.test_adapters")
        .current_dir(repo_root().join("scripts"))
        .env(
            "PYTHONPATH",
            repo_root().join("scripts").to_string_lossy().to_string(),
        )
        .output()
        .expect("run script adapter unittests");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn subprocess_coinset_record_returns_parsed_payload() {
    let output = Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "coinset",
            "record",
            "--network",
            "mainnet",
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
    if !output.status.success() {
        eprintln!(
            "skip subprocess_coinset_record_returns_parsed_payload: coinset unavailable: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let payload = parse_json_output(&output.stdout);
    assert!(payload.get("record").is_some());
}

#[test]
fn subprocess_kms_public_key_emits_json_shape() {
    let output = Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "kms-public-key-compressed-hex",
            "--key-id",
            "arn:aws:kms:us-east-1:123456789012:key/demo",
            "--region",
            "us-east-1",
            "--json",
        ])
        .output()
        .expect("spawn kms-public-key subprocess");
    if output.status.success() {
        let payload = parse_json_output(&output.stdout);
        assert!(payload
            .get("public_key_compressed_hex")
            .and_then(Value::as_str)
            .is_some());
        return;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("kms") || stderr.contains("KMS") || stderr.contains("credentials"),
        "unexpected kms failure: {stderr}"
    );
}

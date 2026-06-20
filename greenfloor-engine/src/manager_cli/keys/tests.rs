use std::fs;

use super::{
    discover_chia_keys, prefers_existing_chia_keys, run_keys_onboard, save_key_onboarding_selection,
};
use crate::manager_cli::test_support::{copy_example_program, pop_json, ManagerContextBuilder};
use serde_json::{json, Value};

#[test]
fn prefers_existing_chia_keys_defaults_and_yes_variants() {
    assert!(prefers_existing_chia_keys(""));
    assert!(prefers_existing_chia_keys("Y"));
    assert!(prefers_existing_chia_keys("yes"));
    assert!(!prefers_existing_chia_keys("n"));
    assert!(!prefers_existing_chia_keys("no"));
}

#[test]
fn discover_chia_keys_detects_keyring_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let keyring = dir.path().join("keyring.yaml");
    fs::write(&keyring, "keys: []\n").expect("write keyring");
    let discovery = discover_chia_keys(Some(dir.path()));
    assert!(discovery.has_existing_keys);
    assert_eq!(discovery.keyring_yaml_path, keyring);
}

#[test]
fn discover_chia_keys_handles_missing_keyring_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let discovery = discover_chia_keys(Some(dir.path()));
    assert!(!discovery.has_existing_keys);
}

#[test]
fn save_key_onboarding_selection_writes_json_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("key_onboarding.json");
    let payload = json!({
        "selected_source": "chia_keys",
        "key_id": "key-main-1",
        "network": "mainnet",
    });
    let written = save_key_onboarding_selection(&path, &payload).expect("save");
    assert_eq!(written, path);
    let text = fs::read_to_string(&path).expect("read");
    let loaded: Value = serde_json::from_str(&text).expect("json");
    assert_eq!(
        loaded.get("selected_source").and_then(Value::as_str),
        Some("chia_keys")
    );
    assert_eq!(
        loaded.get("key_id").and_then(Value::as_str),
        Some("key-main-1")
    );
}

#[test]
fn keys_onboard_import_words_records_selection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = copy_example_program(dir.path());
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("create state");
    let no_keys_dir = dir.path().join("no-keys");
    std::fs::create_dir_all(&no_keys_dir).expect("create no-keys");
    let mnemonic = (1..=12)
        .map(|i| format!("word{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let harness = ManagerContextBuilder::new(program, dir.path().join("unused-markets.yaml"))
        .scratch_dir(dir.path().to_path_buf())
        .prompt_lines(&["1", &mnemonic])
        .build_capturing();
    let code = run_keys_onboard(
        &harness.ctx,
        "key-main-1",
        &state_dir,
        Some(no_keys_dir.as_path()),
    )
    .expect("keys-onboard");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(
        payload.get("selected_source"),
        Some(&json!("mnemonic_import"))
    );
    assert_eq!(payload.get("mnemonic_word_count"), Some(&json!(12)));
    assert!(state_dir.join("key_onboarding.json").is_file());
}

#[test]
fn keys_onboard_import_words_rejects_non_12_or_24_word_secret() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = copy_example_program(dir.path());
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("create state");
    let no_keys_dir = dir.path().join("no-keys");
    std::fs::create_dir_all(&no_keys_dir).expect("create no-keys");
    let ctx = ManagerContextBuilder::new(program, dir.path().join("unused-markets.yaml"))
        .scratch_dir(dir.path().to_path_buf())
        .prompt_lines(&["1", "not enough words"])
        .json_compact(false)
        .build();
    let err = run_keys_onboard(&ctx, "key-main-1", &state_dir, Some(no_keys_dir.as_path()))
        .expect_err("invalid mnemonic");
    assert!(err
        .to_string()
        .contains("mnemonic must contain 12 or 24 words"));
}

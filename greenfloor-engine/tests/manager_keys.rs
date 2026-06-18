#[path = "fixtures/json_util.rs"]
mod json_util;
#[path = "fixtures/manager.rs"]
mod manager_fixtures;

use manager_fixtures::{copy_example_program_and_markets, parse_json_output, run_manager};
use serde_json::json;

#[test]
fn keys_onboard_import_words_records_selection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (program, _) = copy_example_program_and_markets(dir.path());
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("create state");
    let no_keys_dir = dir.path().join("no-keys");
    std::fs::create_dir_all(&no_keys_dir).expect("create no-keys");
    let mnemonic = (1..=12)
        .map(|i| format!("word{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "keys-onboard",
            "--key-id",
            "key-main-1",
            "--state-dir",
            state_dir.to_str().expect("state dir"),
            "--chia-keys-dir",
            no_keys_dir.to_str().expect("no keys"),
        ],
        None,
        Some(&format!("1\n{mnemonic}\n")),
    );
    assert_eq!(output.status.code(), Some(0));
    let payload = parse_json_output(&output.stdout);
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
    let (program, _) = copy_example_program_and_markets(dir.path());
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("create state");
    let no_keys_dir = dir.path().join("no-keys");
    std::fs::create_dir_all(&no_keys_dir).expect("create no-keys");
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "keys-onboard",
            "--key-id",
            "key-main-1",
            "--state-dir",
            state_dir.to_str().expect("state dir"),
            "--chia-keys-dir",
            no_keys_dir.to_str().expect("no keys"),
        ],
        None,
        Some("1\nnot enough words\n"),
    );
    assert_eq!(output.status.code(), Some(1));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("mnemonic must contain 12 or 24 words"));
}

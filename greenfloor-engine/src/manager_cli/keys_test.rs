use super::run_keys_onboard;
use crate::manager_cli::test_support::{
    copy_example_program, pop_json, ManagerContextBuilder, TestPromptLines,
};

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
    let _prompts = TestPromptLines::new(vec!["1", &mnemonic]);
    let harness = ManagerContextBuilder::new(program, dir.path().join("unused-markets.yaml"))
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
        Some(&serde_json::json!("mnemonic_import"))
    );
    assert_eq!(
        payload.get("mnemonic_word_count"),
        Some(&serde_json::json!(12))
    );
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
    let _prompts = TestPromptLines::new(vec!["1", "not enough words"]);
    let ctx = ManagerContextBuilder::new(program, dir.path().join("unused-markets.yaml"))
        .json_compact(false)
        .build();
    let err = run_keys_onboard(&ctx, "key-main-1", &state_dir, Some(no_keys_dir.as_path()))
        .expect_err("invalid mnemonic");
    assert!(err
        .to_string()
        .contains("mnemonic must contain 12 or 24 words"));
}

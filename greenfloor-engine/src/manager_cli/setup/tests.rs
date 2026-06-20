use super::{
    run_bootstrap_home, run_cats_fields, run_config_validate, run_doctor, run_markets_fields,
    run_materialize_minimal_program, run_program_fields, BootstrapHomeParams,
    MaterializeMinimalProgramFeatureFlags, MaterializeMinimalProgramRequest,
};
use crate::manager_cli::test_support::{
    copy_bootstrap_templates, copy_example_program_and_markets, pop_json, repo_root,
    ManagerContextBuilder, TestRuntimeOverrides,
};

#[test]
fn validate_log_level_accepts_info() {
    assert_eq!(
        crate::file_logging::validate_log_level("info").expect("level"),
        "INFO"
    );
}

#[test]
fn validate_log_level_rejects_garbage() {
    assert!(crate::file_logging::validate_log_level("verbose").is_err());
}

#[test]
fn config_validate_emits_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program_path = dir.path().join("program.yaml");
    let markets_path = dir.path().join("markets.yaml");
    std::fs::write(
        &program_path,
        r#"app:
  network: mainnet
  home_dir: /tmp/gf
runtime:
  loop_interval_seconds: 30
chain_signals:
  tx_block_trigger:
    mode: websocket
dev:
  python:
    min_version: "3.11"
notifications:
  low_inventory_alerts:
    enabled: true
    threshold_mode: absolute_base_units
    default_threshold_base_units: 0
    dedup_cooldown_seconds: 21600
    clear_hysteresis_percent: 10
  providers:
    - type: pushover
      enabled: true
      user_key_env: PUSHOVER_USER_KEY
      app_token_env: PUSHOVER_APP_TOKEN
      recipient_key_env: PUSHOVER_RECIPIENT_KEY
"#,
    )
    .expect("write program");
    std::fs::write(&markets_path, "markets: []\n").expect("write markets");
    let ctx = ManagerContextBuilder::new(program_path, markets_path)
        .json_compact(false)
        .build();
    let code = run_config_validate(&ctx, false).expect("validate");
    assert_eq!(code, 0);
}

#[test]
fn program_fields_reads_example_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
    let harness = ManagerContextBuilder::new(program, dir.path().join("unused-markets.yaml"))
        .cats_config(dir.path().join("unused-cats.yaml"))
        .build_capturing();
    let code = run_program_fields(&harness.ctx).expect("program-fields");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(
        payload.get("network").and_then(serde_json::Value::as_str),
        Some("mainnet")
    );
    let registry = payload
        .get("keys_registry")
        .and_then(serde_json::Value::as_object)
        .expect("keys registry");
    assert!(registry.contains_key("key-main-1"));
}

#[test]
fn markets_fields_reads_example_markets() {
    let harness = ManagerContextBuilder::new(
        repo_root().join("config/program.yaml"),
        repo_root().join("config/markets.yaml"),
    )
    .testnet_markets(repo_root().join("config/testnet-markets.yaml"))
    .build_capturing();
    let code = run_markets_fields(&harness.ctx).expect("markets-fields");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    let enabled = payload
        .get("enabled_markets")
        .and_then(|v| v.as_array())
        .expect("enabled markets");
    assert!(!enabled.is_empty());
    assert!(enabled.iter().all(|row| {
        row.get("enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    }));
}

#[test]
fn cats_fields_reads_example_cats() {
    let harness = ManagerContextBuilder::new(
        repo_root().join("config/program.yaml"),
        repo_root().join("config/markets.yaml"),
    )
    .cats_config(repo_root().join("config/cats.yaml"))
    .build_capturing();
    let code = run_cats_fields(&harness.ctx).expect("cats-fields");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    let symbol_map = payload
        .get("symbol_to_asset_id")
        .and_then(serde_json::Value::as_object)
        .expect("symbol_to_asset_id map");
    assert!(!symbol_map.is_empty());
}

#[test]
fn config_validate_accepts_example_configs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
    std::fs::copy(repo_root().join("config/markets.yaml"), &markets).expect("copy markets");
    let harness = ManagerContextBuilder::new(program, markets)
        .cats_config(dir.path().join("unused-cats.yaml"))
        .build_capturing();
    let code = run_config_validate(&harness.ctx, false).expect("config-validate");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(
        payload.get("ok").and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test]
fn config_validate_program_only_accepts_example_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
    let ctx = ManagerContextBuilder::new(program, dir.path().join("unused-markets.yaml"))
        .json_compact(false)
        .build();
    let code = run_config_validate(&ctx, true).expect("config-validate program-only");
    assert_eq!(code, 0);
}

#[test]
fn materialize_minimal_program_template_writes_expected_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let program = dir.path().join("program.yaml");
    let code = run_materialize_minimal_program(MaterializeMinimalProgramRequest {
        output: &program,
        home_dir: &home,
        dexie_api_base: "https://dexie.test",
        log_level: "INFO",
        features: MaterializeMinimalProgramFeatureFlags {
            dry_run: false,
            low_inventory_alerts_enabled: true,
            pushover_enabled: true,
        },
        with_signer: false,
    });
    assert_eq!(code, 0);
    let raw: serde_json::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&program).expect("read program"))
            .expect("parse yaml");
    assert_eq!(
        raw.get("app")
            .and_then(|app| app.get("home_dir"))
            .and_then(serde_json::Value::as_str),
        Some(home.to_str().expect("home path"))
    );
    assert_eq!(
        raw.get("venues")
            .and_then(|venues| venues.get("dexie"))
            .and_then(|dexie| dexie.get("api_base"))
            .and_then(serde_json::Value::as_str),
        Some("https://dexie.test")
    );
    assert_eq!(
        raw.get("dev")
            .and_then(|dev| dev.get("python"))
            .and_then(|python| python.get("min_version"))
            .and_then(serde_json::Value::as_str),
        Some("3.11")
    );
}

fn bootstrap_home_in_process(
    home_dir: &std::path::Path,
    program_template: &std::path::Path,
    markets_template: &std::path::Path,
    cats_template: &std::path::Path,
    testnet_markets_template: &std::path::Path,
    seed_testnet_markets: bool,
    force: bool,
) -> i32 {
    let ctx = ManagerContextBuilder::new(
        program_template.to_path_buf(),
        markets_template.to_path_buf(),
    )
    .json_compact(false)
    .build();
    run_bootstrap_home(&BootstrapHomeParams {
        ctx: &ctx,
        home_dir,
        program_template,
        markets_template,
        cats_template: Some(cats_template),
        testnet_markets_template: Some(testnet_markets_template),
        seed_testnet_markets,
        force,
    })
    .expect("bootstrap-home")
}

#[test]
fn bootstrap_home_creates_layout_and_seed_configs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home_dir = dir.path().join(".greenfloor");
    let (program_template, markets_template, cats_template, testnet_markets_template) =
        copy_bootstrap_templates(dir.path());
    assert_eq!(
        bootstrap_home_in_process(
            &home_dir,
            &program_template,
            &markets_template,
            &cats_template,
            &testnet_markets_template,
            false,
            false,
        ),
        0
    );
    assert!(home_dir.join("config").is_dir());
    assert!(home_dir.join("db").is_dir());
    assert!(home_dir.join("state").is_dir());
    assert!(home_dir.join("logs").is_dir());
    assert!(home_dir.join("db").join("greenfloor.sqlite").is_file());
    assert!(home_dir.join("config").join("program.yaml").is_file());
    assert!(home_dir.join("config").join("markets.yaml").is_file());
    assert!(home_dir.join("config").join("cats.yaml").is_file());
}

#[test]
fn bootstrap_home_without_force_keeps_existing_seeded_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home_dir = dir.path().join(".greenfloor");
    let config_dir = home_dir.join("config");
    std::fs::create_dir_all(&config_dir).expect("create config");
    std::fs::write(
        config_dir.join("program.yaml"),
        "app:\n  home_dir: \"custom-home\"\n",
    )
    .expect("write program");
    std::fs::write(config_dir.join("markets.yaml"), "markets: []\n").expect("write markets");
    std::fs::write(config_dir.join("cats.yaml"), "cats: []\n").expect("write cats");
    let (program_template, markets_template, cats_template, testnet_markets_template) =
        copy_bootstrap_templates(dir.path());
    assert_eq!(
        bootstrap_home_in_process(
            &home_dir,
            &program_template,
            &markets_template,
            &cats_template,
            &testnet_markets_template,
            false,
            false,
        ),
        0
    );
    assert_eq!(
        std::fs::read_to_string(config_dir.join("program.yaml")).expect("read program"),
        "app:\n  home_dir: \"custom-home\"\n"
    );
}

#[test]
fn bootstrap_home_can_seed_optional_testnet_markets() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home_dir = dir.path().join(".greenfloor");
    let (program_template, markets_template, cats_template, testnet_markets_template) =
        copy_bootstrap_templates(dir.path());
    assert_eq!(
        bootstrap_home_in_process(
            &home_dir,
            &program_template,
            &markets_template,
            &cats_template,
            &testnet_markets_template,
            true,
            false,
        ),
        0
    );
    assert!(home_dir
        .join("config")
        .join("testnet-markets.yaml")
        .is_file());
}

#[test]
fn doctor_reports_ok_with_example_configs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (program, markets) = copy_example_program_and_markets(dir.path());
    let state_db = dir.path().join("state.sqlite");
    let harness = ManagerContextBuilder::new(program, markets)
        .state_db(state_db.to_str().expect("state db"))
        .build_capturing();
    let code = run_doctor(&harness.ctx).expect("doctor");
    assert_eq!(code, 0);
}

#[test]
fn doctor_fails_when_enabled_market_key_missing_from_registry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (program, markets) = copy_example_program_and_markets(dir.path());
    let markets_text = std::fs::read_to_string(&markets).expect("read markets");
    let patched = markets_text.replace("signer_key_id:", "signer_key_id: \"\" #");
    std::fs::write(&markets, patched).expect("patch markets");
    let state_db = dir.path().join("state.sqlite");
    let harness = ManagerContextBuilder::new(program, markets)
        .state_db(state_db.to_str().expect("state db"))
        .build_capturing();
    let code = run_doctor(&harness.ctx).expect("doctor");
    assert_eq!(code, 2);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("ok"), Some(&serde_json::json!(false)));
    let problems = payload
        .get("problems")
        .and_then(|v| v.as_array())
        .expect("problems");
    assert!(problems.iter().any(|problem| {
        problem
            .as_str()
            .is_some_and(|text| text.contains("missing signer_key_id"))
    }));
}

#[test]
fn doctor_warns_on_invalid_runtime_override_env() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (program, markets) = copy_example_program_and_markets(dir.path());
    let state_db = dir.path().join("state.sqlite");
    let _overrides = TestRuntimeOverrides::new(&[
        ("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "0"),
        ("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", "bad"),
    ]);
    let harness = ManagerContextBuilder::new(program, markets)
        .state_db(state_db.to_str().expect("state db"))
        .build_capturing();
    let code = run_doctor(&harness.ctx).expect("doctor");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    let warnings = payload
        .get("warnings")
        .and_then(|v| v.as_array())
        .expect("warnings");
    assert!(warnings.iter().any(|warning| {
        warning
            .as_str()
            .is_some_and(|text| text.contains("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS"))
    }));
    assert!(warnings.iter().any(|warning| {
        warning
            .as_str()
            .is_some_and(|text| text.contains("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS"))
    }));
}

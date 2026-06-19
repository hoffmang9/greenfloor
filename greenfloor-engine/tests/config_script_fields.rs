use serde_json::Value;

#[path = "fixtures/manager.rs"]
mod manager_fixtures;

use manager_fixtures::{copy_example_program_and_markets, repo_root, run_manager};

fn parse_json_output(stdout: &[u8]) -> Value {
    let text = std::str::from_utf8(stdout).expect("utf8 stdout").trim();
    let start = text.find('{').unwrap_or(0);
    serde_json::from_str(&text[start..]).expect("parse manager json stdout")
}

#[test]
fn manager_program_fields_reads_example_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program path"),
            "--json",
            "program-fields",
        ],
        None,
        None,
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let payload = parse_json_output(&output.stdout);
    assert_eq!(
        payload.get("network").and_then(Value::as_str),
        Some("mainnet")
    );
    let registry = payload
        .get("keys_registry")
        .and_then(Value::as_object)
        .expect("keys registry");
    assert!(registry.contains_key("key-main-1"));
}

#[test]
fn manager_markets_fields_reads_example_markets() {
    let output = run_manager(
        &[
            "--markets-config",
            repo_root()
                .join("config/markets.yaml")
                .to_str()
                .expect("markets path"),
            "--testnet-markets-config",
            repo_root()
                .join("config/testnet-markets.yaml")
                .to_str()
                .expect("testnet markets path"),
            "--json",
            "markets-fields",
        ],
        None,
        None,
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let payload = parse_json_output(&output.stdout);
    let enabled = payload
        .get("enabled_markets")
        .and_then(Value::as_array)
        .expect("enabled markets");
    assert!(!enabled.is_empty());
    assert!(enabled
        .iter()
        .all(|row| { row.get("enabled").and_then(Value::as_bool).unwrap_or(false) }));
}

#[test]
fn manager_cats_fields_reads_example_cats() {
    let output = run_manager(
        &[
            "--cats-config",
            repo_root()
                .join("config/cats.yaml")
                .to_str()
                .expect("cats path"),
            "--json",
            "cats-fields",
        ],
        None,
        None,
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let payload = parse_json_output(&output.stdout);
    let symbol_map = payload
        .get("symbol_to_asset_id")
        .and_then(Value::as_object)
        .expect("symbol_to_asset_id map");
    assert!(!symbol_map.is_empty());
}

#[test]
fn manager_config_validate_accepts_example_configs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (program, markets) = copy_example_program_and_markets(dir.path());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program path"),
            "--markets-config",
            markets.to_str().expect("markets path"),
            "config-validate",
        ],
        None,
        None,
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(true));
}

#[test]
fn manager_config_validate_program_only_accepts_example_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (program, _markets) = copy_example_program_and_markets(dir.path());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program path"),
            "config-validate",
            "--program-only",
        ],
        None,
        None,
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
}

#[test]
fn manager_materialize_minimal_program_template_writes_expected_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let program = dir.path().join("program.yaml");
    let output = run_manager(
        &[
            "materialize-minimal-program",
            "--output",
            program.to_str().expect("program path"),
            "--home-dir",
            home.to_str().expect("home path"),
            "--dexie-api-base",
            "https://dexie.test",
        ],
        None,
        None,
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let raw: Value =
        serde_yaml::from_str(&std::fs::read_to_string(&program).expect("read program"))
            .expect("parse yaml");
    assert_eq!(
        raw.get("app")
            .and_then(|app| app.get("home_dir"))
            .and_then(Value::as_str),
        Some(home.to_str().expect("home path"))
    );
    assert_eq!(
        raw.get("venues")
            .and_then(|venues| venues.get("dexie"))
            .and_then(|dexie| dexie.get("api_base"))
            .and_then(Value::as_str),
        Some("https://dexie.test")
    );
    assert_eq!(
        raw.get("dev")
            .and_then(|dev| dev.get("python"))
            .and_then(|python| python.get("min_version"))
            .and_then(Value::as_str),
        Some("3.11")
    );
}

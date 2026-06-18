#[path = "fixtures/json_util.rs"]
mod json_util;
#[path = "fixtures/manager.rs"]
mod manager_fixtures;

use std::path::{Path, PathBuf};

use greenfloor_engine::storage::SqliteStore;
use manager_fixtures::{
    copy_example_program_and_markets, parse_json_output, patch_program_dexie_base,
    restore_program_dexie_base, run_manager, write_manager_program,
    write_manager_program_with_signer, write_markets_one, write_markets_with_ladder,
};
use serde_json::json;

fn run_doctor(
    program: &Path,
    markets: &Path,
    state_db: &Path,
    env: Option<&[(&str, &str)]>,
) -> (i32, serde_json::Value) {
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "--state-db",
            state_db.to_str().expect("state db"),
            "doctor",
        ],
        env,
        None,
    );
    (
        output.status.code().unwrap_or(-1),
        parse_json_output(&output.stdout),
    )
}

#[test]
fn doctor_reports_ok_with_example_configs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (program, markets) = copy_example_program_and_markets(dir.path());
    let state_db = dir.path().join("state.sqlite");
    let (code, _payload) = run_doctor(&program, &markets, &state_db, None);
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
    let (code, payload) = run_doctor(&program, &markets, &state_db, None);
    assert_eq!(code, 2);
    assert_eq!(payload.get("ok"), Some(&json!(false)));
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
    let env = [
        ("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "0"),
        ("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", "bad"),
    ];
    let (code, payload) = run_doctor(&program, &markets, &state_db, Some(&env));
    assert_eq!(code, 0);
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

fn write_bootstrap_templates(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let program_template = root.join("program.template.yaml");
    let markets_template = root.join("markets.template.yaml");
    let cats_template = root.join("cats.template.yaml");
    let testnet_markets_template = root.join("testnet-markets.template.yaml");
    std::fs::write(
        &program_template,
        r#"app:
  network: "mainnet"
  home_dir: "~/.greenfloor"
runtime:
  loop_interval_seconds: 30
notifications:
  low_inventory_alerts:
    enabled: true
    threshold_mode: "absolute_base_units"
    default_threshold_base_units: 0
    dedup_cooldown_seconds: 3600
    clear_hysteresis_percent: 10
  providers:
    - type: pushover
      enabled: false
      user_key_env: "PUSHOVER_USER_KEY"
      app_token_env: "PUSHOVER_APP_TOKEN"
      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"
chain_signals:
  tx_block_trigger:
    webhook_enabled: true
    webhook_listen_addr: "127.0.0.1:8787"
"#,
    )
    .expect("write program template");
    std::fs::write(
        &markets_template,
        r#"markets:
  - id: m1
    enabled: true
    base_asset: "a1"
    base_symbol: "A1"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 100
"#,
    )
    .expect("write markets template");
    std::fs::write(
        &cats_template,
        r#"cats:
  - name: Token One
    base_symbol: "TOK1"
    asset_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    target_usd_per_unit: null
    dexie:
      ticker_id: null
      pool_id: null
      last_price_xch: null
"#,
    )
    .expect("write cats template");
    std::fs::write(
        &testnet_markets_template,
        r#"markets:
  - id: m-testnet
    enabled: true
    base_asset: "ta1"
    base_symbol: "TA1"
    quote_asset: "txch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "txch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 100
"#,
    )
    .expect("write testnet template");
    (
        program_template,
        markets_template,
        cats_template,
        testnet_markets_template,
    )
}

fn bootstrap_home(
    home_dir: &Path,
    program_template: &Path,
    markets_template: &Path,
    cats_template: &Path,
    testnet_markets_template: &Path,
    seed_testnet_markets: bool,
    force: bool,
) -> i32 {
    let mut args = vec![
        "bootstrap-home",
        "--home-dir",
        home_dir.to_str().expect("home"),
        "--program-template",
        program_template.to_str().expect("program template"),
        "--markets-template",
        markets_template.to_str().expect("markets template"),
        "--cats-template",
        cats_template.to_str().expect("cats template"),
        "--testnet-markets-template",
        testnet_markets_template.to_str().expect("testnet template"),
    ];
    if seed_testnet_markets {
        args.push("--seed-testnet-markets");
    }
    if force {
        args.push("--force");
    }
    run_manager(&args, None, None).status.code().unwrap_or(-1)
}

#[test]
fn bootstrap_home_creates_layout_and_seed_configs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home_dir = dir.path().join(".greenfloor");
    let (program_template, markets_template, cats_template, testnet_markets_template) =
        write_bootstrap_templates(dir.path());
    assert_eq!(
        bootstrap_home(
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
        write_bootstrap_templates(dir.path());
    assert_eq!(
        bootstrap_home(
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
        write_bootstrap_templates(dir.path());
    assert_eq!(
        bootstrap_home(
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
fn coins_list_requires_signer_backend() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_manager_program(&program, dir.path());
    write_markets_one(&markets);
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "coins-list",
        ],
        None,
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(
        payload.get("error"),
        Some(&json!("coin_list_requires_signer_backend"))
    );
}

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

#[test]
fn build_and_post_offer_dry_run_returns_preview() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_manager_program_with_signer(&program, dir.path());
    let markets_yaml = r#"markets:
  - id: m1
    enabled: true
    base_asset: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    base_symbol: "TCAT"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#;
    std::fs::write(&markets, markets_yaml).expect("write markets");
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "build-and-post-offer",
            "--market-id",
            "m1",
            "--size-base-units",
            "1",
            "--dry-run",
            "--network",
            "mainnet",
        ],
        Some(&[("GREENFLOOR_TEST_OFFER_TEXT", "offer1dryrunpreviewstub")]),
        None,
    );
    assert_eq!(output.status.code(), Some(0));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("dry_run"), Some(&json!(true)));
    assert_eq!(payload.get("publish_attempts"), Some(&json!(0)));
    assert!(payload
        .get("built_offers_preview")
        .and_then(|v| v.as_array())
        .is_some_and(|rows| !rows.is_empty()));
    assert_eq!(payload.get("results"), Some(&json!([])));
}

fn seed_offer_states(db_path: &Path, rows: &[(&str, &str, &str)]) {
    let store = SqliteStore::open(db_path).expect("open db");
    for (offer_id, market_id, state) in rows {
        store
            .upsert_offer_state(offer_id, market_id, state, Some(0))
            .expect("seed offer");
    }
}

#[tokio::test]
async fn offers_reconcile_updates_states_from_dexie() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let db_path = dir.path().join("state.sqlite");
    write_manager_program(&program, dir.path());
    let confirmed_tx_id = "a".repeat(64);
    {
        let store = SqliteStore::open(&db_path).expect("open db");
        store
            .upsert_offer_state("offer-ok", "m1", "open", Some(0))
            .expect("seed");
        store
            .upsert_offer_state("offer-missing", "m1", "open", Some(0))
            .expect("seed");
        store
            .observe_mempool_tx_ids(std::slice::from_ref(&confirmed_tx_id))
            .expect("observe");
        store.confirm_tx_ids(&[confirmed_tx_id]).expect("confirm");
    }

    let mut server = mockito::Server::new_async().await;
    let confirmed_tx_id = "a".repeat(64);
    let _ok = server
        .mock("GET", "/v1/offers/offer-ok")
        .with_status(200)
        .with_body(json!({"id":"offer-ok","status":4,"tx_id": confirmed_tx_id}).to_string())
        .create_async()
        .await;
    let _missing = server
        .mock("GET", "/v1/offers/offer-missing")
        .with_status(404)
        .with_body(r#"{"success":false,"error":"not_found"}"#)
        .create_async()
        .await;
    let original = std::fs::read_to_string(&program).expect("read program");
    patch_program_dexie_base(&program, &server.url());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--state-db",
            db_path.to_str().expect("db"),
            "offers-reconcile",
            "--limit",
            "20",
            "--venue",
            "dexie",
        ],
        None,
        None,
    );
    restore_program_dexie_base(&program, &original);
    assert_eq!(output.status.code(), Some(0));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("reconciled_count"), Some(&json!(2)));
    assert_eq!(payload.get("changed_count"), Some(&json!(2)));

    let store = SqliteStore::open(&db_path).expect("open db");
    let rows = store.list_offer_states(None, 20).expect("rows");
    let by_id = rows
        .iter()
        .map(|row| (row.offer_id.as_str(), row))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        by_id.get("offer-ok").expect("offer-ok").state,
        "tx_block_confirmed"
    );
    assert_eq!(
        by_id.get("offer-missing").expect("missing").state,
        "expired"
    );
}

#[tokio::test]
async fn offers_cancel_cancel_open_uses_dexie() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    write_manager_program(&program, dir.path());
    seed_offer_states(
        &db_path,
        &[
            ("offer-open", "m1", "open"),
            ("offer-expired", "m1", "expired"),
        ],
    );

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/v1/offers/offer-open/cancel")
        .with_status(200)
        .with_body(r#"{"success":true,"id":"offer-open","status":3}"#)
        .create_async()
        .await;
    let original = std::fs::read_to_string(&program).expect("read program");
    patch_program_dexie_base(&program, &server.url());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "offers-cancel",
            "--cancel-open",
        ],
        None,
        None,
    );
    restore_program_dexie_base(&program, &original);
    assert_eq!(output.status.code(), Some(0));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("venue"), Some(&json!("dexie")));
    assert_eq!(payload.get("selected_count"), Some(&json!(1)));
    assert_eq!(payload.get("cancelled_count"), Some(&json!(1)));
    assert_eq!(payload.get("failed_count"), Some(&json!(0)));

    let store = SqliteStore::open(&db_path).expect("open db");
    let rows = store.list_offer_states(None, 10).expect("rows");
    let by_id = rows
        .iter()
        .map(|row| (row.offer_id.as_str(), row))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(by_id.get("offer-open").expect("open").state, "cancelled");
    assert_eq!(
        by_id.get("offer-open").expect("open").last_seen_status,
        Some(3)
    );
    assert_eq!(
        by_id.get("offer-expired").expect("expired").state,
        "expired"
    );
}

#[tokio::test]
async fn offers_cancel_by_offer_id_uses_dexie() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    write_manager_program(&program, dir.path());
    seed_offer_states(
        &db_path,
        &[
            ("offer-target", "m1", "open"),
            ("offer-other", "m1", "open"),
        ],
    );

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/v1/offers/offer-target/cancel")
        .with_status(200)
        .with_body(r#"{"success":true,"id":"offer-target","status":3}"#)
        .create_async()
        .await;
    let original = std::fs::read_to_string(&program).expect("read program");
    patch_program_dexie_base(&program, &server.url());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "offers-cancel",
            "--offer-id",
            "offer-target",
        ],
        None,
        None,
    );
    restore_program_dexie_base(&program, &original);
    assert_eq!(output.status.code(), Some(0));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("selected_count"), Some(&json!(1)));
    assert_eq!(payload.get("cancelled_count"), Some(&json!(1)));
}

#[tokio::test]
async fn offers_cancel_reports_dexie_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    write_manager_program(&program, dir.path());
    seed_offer_states(&db_path, &[("offer-fail", "m1", "open")]);

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/v1/offers/offer-fail/cancel")
        .with_status(200)
        .with_body(r#"{"success":false,"error":"not_found"}"#)
        .create_async()
        .await;
    let original = std::fs::read_to_string(&program).expect("read program");
    patch_program_dexie_base(&program, &server.url());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "offers-cancel",
            "--offer-id",
            "offer-fail",
        ],
        None,
        None,
    );
    restore_program_dexie_base(&program, &original);
    assert_eq!(output.status.code(), Some(2));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("cancelled_count"), Some(&json!(0)));
    assert_eq!(payload.get("failed_count"), Some(&json!(1)));
}

#[test]
fn offers_cancel_rejects_removed_submit_onchain_flag() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    write_manager_program(&program, dir.path());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "offers-cancel",
            "--offer-id",
            "offer-1",
            "--submit-onchain-after-offchain",
        ],
        None,
        None,
    );
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn coin_split_until_ready_requires_size_base_units() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_manager_program_with_signer(&program, dir.path());
    write_markets_with_ladder(&markets);
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "coin-split",
            "--market-id",
            "m1",
            "--until-ready",
            "--network",
            "mainnet",
        ],
        None,
        None,
    );
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("until-ready mode requires --size-base-units"));
}

#[test]
fn coin_split_until_ready_disallows_no_wait() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_manager_program_with_signer(&program, dir.path());
    write_markets_with_ladder(&markets);
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "coin-split",
            "--market-id",
            "m1",
            "--until-ready",
            "--size-base-units",
            "10",
            "--no-wait",
            "--network",
            "mainnet",
        ],
        None,
        None,
    );
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("until-ready mode requires wait mode"));
}

#[test]
fn coin_combine_until_ready_requires_size_base_units() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_manager_program_with_signer(&program, dir.path());
    write_markets_with_ladder(&markets);
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "coin-combine",
            "--market-id",
            "m1",
            "--until-ready",
            "--network",
            "mainnet",
        ],
        None,
        None,
    );
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("until-ready mode requires --size-base-units"));
}

fn cats_list(cats_path: &Path) -> serde_json::Value {
    let output = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-list",
        ],
        None,
        None,
    );
    assert_eq!(output.status.code(), Some(0));
    parse_json_output(&output.stdout)
}

#[test]
fn cats_add_manual_without_dexie_lookup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cats_path = dir.path().join("cats.yaml");
    let output = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "--name",
            "Manual CAT",
            "--base-symbol",
            "MCAT",
            "--ticker-id",
            "manualcat_xch",
            "--pool-id",
            "pool-manual",
            "--last-price-xch",
            "0.42",
            "--target-usd-per-unit",
            "4.2",
            "--no-dexie-lookup",
        ],
        None,
        None,
    );
    assert_eq!(output.status.code(), Some(0));
    let payload = cats_list(&cats_path);
    let rows = payload
        .get("cats")
        .and_then(|v| v.as_array())
        .expect("cats");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.get("name"), Some(&json!("Manual CAT")));
    assert_eq!(row.get("base_symbol"), Some(&json!("MCAT")));
    assert_eq!(
        row.get("asset_id"),
        Some(&json!(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ))
    );
    let add_payload = parse_json_output(&output.stdout);
    assert_eq!(add_payload.get("added"), Some(&json!(true)));
}

#[test]
fn cats_add_replace_required_for_existing_asset() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cats_path = dir.path().join("cats.yaml");
    let cat_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let first = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "First",
            "--base-symbol",
            "ONE",
            "--no-dexie-lookup",
        ],
        None,
        None,
    );
    assert_eq!(first.status.code(), Some(0));
    let second = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "Second",
            "--base-symbol",
            "TWO",
            "--no-dexie-lookup",
        ],
        None,
        None,
    );
    assert_eq!(second.status.code(), Some(2));
    let payload = parse_json_output(&second.stdout);
    assert_eq!(payload.get("error"), Some(&json!("cat_already_exists")));
}

#[test]
fn cats_delete_by_cat_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cats_path = dir.path().join("cats.yaml");
    let cat_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    assert_eq!(
        run_manager(
            &[
                "--cats-config",
                cats_path.to_str().expect("cats"),
                "cats-add",
                "--network",
                "mainnet",
                "--cat-id",
                cat_id,
                "--name",
                "Delete Me",
                "--base-symbol",
                "DEL",
                "--no-dexie-lookup",
            ],
            None,
            None,
        )
        .status
        .code(),
        Some(0)
    );
    let deleted = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-delete",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--yes",
        ],
        None,
        None,
    );
    assert_eq!(deleted.status.code(), Some(0));
    let payload = parse_json_output(&deleted.stdout);
    assert_eq!(payload.get("deleted"), Some(&json!(true)));
    assert!(cats_list(&cats_path)
        .get("cats")
        .and_then(|v| v.as_array())
        .is_some_and(|rows| rows.is_empty()));
}

#[test]
fn cats_delete_requires_confirmation_when_not_yes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cats_path = dir.path().join("cats.yaml");
    let cat_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    assert_eq!(
        run_manager(
            &[
                "--cats-config",
                cats_path.to_str().expect("cats"),
                "cats-add",
                "--network",
                "mainnet",
                "--cat-id",
                cat_id,
                "--name",
                "Needs Confirm",
                "--base-symbol",
                "CNF",
                "--no-dexie-lookup",
            ],
            None,
            None,
        )
        .status
        .code(),
        Some(0)
    );
    let deleted = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-delete",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
        ],
        None,
        None,
    );
    assert_eq!(deleted.status.code(), Some(2));
    let payload = parse_json_output(&deleted.stdout);
    assert_eq!(payload.get("error"), Some(&json!("confirmation_required")));
}

#[test]
fn cats_delete_preflight_only_does_not_delete() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cats_path = dir.path().join("cats.yaml");
    let cat_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    assert_eq!(
        run_manager(
            &[
                "--cats-config",
                cats_path.to_str().expect("cats"),
                "cats-add",
                "--network",
                "mainnet",
                "--cat-id",
                cat_id,
                "--name",
                "Preflight Only",
                "--base-symbol",
                "PFL",
                "--no-dexie-lookup",
            ],
            None,
            None,
        )
        .status
        .code(),
        Some(0)
    );
    assert_eq!(
        run_manager(
            &[
                "--cats-config",
                cats_path.to_str().expect("cats"),
                "cats-delete",
                "--network",
                "mainnet",
                "--cat-id",
                cat_id,
                "--preflight-only",
            ],
            None,
            None,
        )
        .status
        .code(),
        Some(0)
    );
    assert_eq!(
        cats_list(&cats_path)
            .get("cats")
            .and_then(|v| v.as_array())
            .map_or(0, |rows| rows.len()),
        1
    );
}

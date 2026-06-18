#[path = "fixtures/json_util.rs"]
mod json_util;
#[path = "fixtures/manager.rs"]
mod manager_fixtures;

use std::path::{Path, PathBuf};

use manager_fixtures::run_manager;

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

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub fn run_manager(args: &[&str], env: Option<&[(&str, &str)]>, stdin: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_greenfloor-manager"));
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(pairs) = env {
        for (key, value) in pairs {
            command.env(key, value);
        }
    }
    let mut child = command.spawn().expect("spawn greenfloor-manager");
    if let Some(input) = stdin {
        if let Some(mut stdin_pipe) = child.stdin.take() {
            stdin_pipe
                .write_all(input.as_bytes())
                .expect("write manager stdin");
            stdin_pipe.flush().expect("flush manager stdin");
        }
    } else if let Some(stdin_pipe) = child.stdin.take() {
        drop(stdin_pipe);
    }
    child
        .wait_with_output()
        .expect("wait for greenfloor-manager")
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

pub fn copy_example_program_and_markets(tmp: &Path) -> (PathBuf, PathBuf) {
    let root = repo_root();
    let program = tmp.join("program.yaml");
    let markets = tmp.join("markets.yaml");
    std::fs::copy(root.join("config/program.yaml"), &program).expect("copy program");
    std::fs::copy(root.join("config/markets.yaml"), &markets).expect("copy markets");
    (program, markets)
}

pub fn write_manager_program(path: &Path, home_dir: &Path) {
    let home = home_dir.display();
    let yaml = format!(
        r#"app:
  network: "mainnet"
  home_dir: "{home}"
runtime:
  loop_interval_seconds: 30
chain_signals:
  tx_block_trigger:
    mode: "websocket"
    webhook_enabled: true
    webhook_listen_addr: "127.0.0.1:8787"
dev:
  python:
    min_version: "3.11"
notifications:
  low_inventory_alerts:
    enabled: true
    threshold_mode: "absolute_base_units"
    default_threshold_base_units: 0
    dedup_cooldown_seconds: 60
    clear_hysteresis_percent: 10
  providers:
    - type: pushover
      enabled: true
      user_key_env: "PUSHOVER_USER_KEY"
      app_token_env: "PUSHOVER_APP_TOKEN"
      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"
venues:
  dexie:
    api_base: "https://api.dexie.space"
  splash:
    api_base: "http://localhost:4000"
  offer_publish:
    provider: "dexie"
"#
    );
    std::fs::write(path, yaml).expect("write manager program");
}

pub fn write_manager_program_with_signer(path: &Path, home_dir: &Path) {
    let root = repo_root();
    let mut text =
        std::fs::read_to_string(root.join("config/program.yaml")).expect("read program template");
    let home = home_dir.display().to_string();
    if text.contains("home_dir: \"~/.greenfloor\"") {
        text = text.replace(
            "home_dir: \"~/.greenfloor\"",
            &format!("home_dir: \"{home}\""),
        );
    } else {
        text = text.replacen("home_dir:", &format!("home_dir: \"{home}\""), 1);
    }
    if text.contains("kms_key_id: \"\"") {
        text = text.replace(
            "kms_key_id: \"\"",
            "kms_key_id: \"arn:aws:kms:us-west-2:123:key/demo\"",
        );
    }
    if text.contains("kms_region: \"\"") {
        text = text.replace("kms_region: \"\"", "kms_region: \"us-west-2\"");
    }
    if text.contains("kms_public_key_hex: \"\"") {
        text = text.replace(
            "kms_public_key_hex: \"\"",
            "kms_public_key_hex: \"02abc123\"",
        );
    }
    std::fs::write(path, text).expect("write signer program");
}

pub fn write_markets_with_ladder(path: &Path) {
    let yaml = r#"markets:
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
      low_watermark_base_units: 10
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
    ladders:
      sell:
        - size_base_units: 10
          target_count: 3
          split_buffer_count: 1
          combine_when_excess_factor: 2.0
"#;
    std::fs::write(path, yaml).expect("write ladder markets");
}

pub fn write_markets_one(path: &Path) {
    let yaml = r#"markets:
  - id: m1
    enabled: true
    base_asset: "asset1"
    base_symbol: "AS1"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
      bucket_counts:
        1: 0
    ladders:
      sell:
        - size_base_units: 1
          target_count: 1
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
"#;
    std::fs::write(path, yaml).expect("write markets yaml");
}

pub fn patch_program_dexie_base(program: &Path, dexie_base: &str) {
    let text = std::fs::read_to_string(program).expect("read program");
    let patched = text.replace("https://api.dexie.space", dexie_base);
    std::fs::write(program, patched).expect("patch dexie base");
}

pub fn restore_program_dexie_base(program: &Path, original: &str) {
    std::fs::write(program, original).expect("restore program");
}

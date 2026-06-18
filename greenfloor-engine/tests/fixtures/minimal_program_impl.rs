use std::path::Path;

pub struct MinimalProgramParams<'a> {
    pub home_dir: &'a Path,
    pub dexie_api_base: &'a str,
    pub log_level: Option<&'a str>,
    pub dry_run: bool,
    pub low_inventory_alerts_enabled: bool,
    pub pushover_enabled: bool,
}

impl<'a> Default for MinimalProgramParams<'a> {
    fn default() -> Self {
        Self {
            home_dir: Path::new("/tmp/greenfloor-test-home"),
            dexie_api_base: "https://api.dexie.space",
            log_level: Some("INFO"),
            dry_run: false,
            low_inventory_alerts_enabled: false,
            pushover_enabled: false,
        }
    }
}

pub fn write_minimal_program(path: &Path, params: MinimalProgramParams<'_>) {
    let home = params.home_dir.display();
    let log_level = params
        .log_level
        .unwrap_or("INFO")
        .trim()
        .to_ascii_uppercase();
    let dry_run = if params.dry_run { "true" } else { "false" };
    let alerts_enabled = if params.low_inventory_alerts_enabled {
        "true"
    } else {
        "false"
    };
    let pushover_enabled = if params.pushover_enabled {
        "true"
    } else {
        "false"
    };
    let yaml = format!(
        r#"app:
  network: "mainnet"
  home_dir: "{home}"
  log_level: {log_level}
runtime:
  loop_interval_seconds: 30
  dry_run: {dry_run}
chain_signals:
  tx_block_trigger:
    mode: "websocket"
    websocket_url: "wss://api.coinset.org/ws"
    websocket_reconnect_interval_seconds: 1
    fallback_poll_interval_seconds: 1
dev:
  python:
    min_version: "3.11"
notifications:
  low_inventory_alerts:
    enabled: {alerts_enabled}
    threshold_mode: "absolute_base_units"
    default_threshold_base_units: 0
    dedup_cooldown_seconds: 60
    clear_hysteresis_percent: 10
  providers:
    - type: pushover
      enabled: {pushover_enabled}
      user_key_env: "PUSHOVER_USER_KEY"
      app_token_env: "PUSHOVER_APP_TOKEN"
      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"
venues:
  dexie:
    api_base: "{dexie}"
  splash:
    api_base: "http://localhost:4000"
  offer_publish:
    provider: "dexie"
coin_ops:
  max_operations_per_run: 0
  max_daily_fee_budget_mojos: 0
  split_fee_mojos: 0
  combine_fee_mojos: 0
keys:
  registry:
    - key_id: "key-main-1"
      fingerprint: 123456789
      network: "mainnet"
      keyring_yaml_path: "~/.chia_keys/keyring.yaml"
"#,
        dexie = params.dexie_api_base
    );
    std::fs::write(path, yaml).expect("write minimal program yaml");
}

#[allow(dead_code)] // used by lib unit tests; integration fixtures include this module too
pub fn write_minimal_program_with_signer(path: &Path, params: MinimalProgramParams<'_>) {
    write_minimal_program(path, params);
    let launcher_id = "aa".repeat(32);
    let signer_block = format!(
        r#"
signer:
  kms_key_id: arn:aws:kms:us-west-2:123:key/abc
  kms_region: us-west-2
vault:
  launcher_id: {launcher_id}
  custody_threshold: 1
  recovery_threshold: 1
  recovery_clawback_timelock: 3600
  custody_keys:
    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"
      curve: SECP256R1
  recovery_keys:
    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb9901baa6b7a99d"
      curve: SECP256R1
"#
    );
    let mut contents = std::fs::read_to_string(path).expect("read minimal program");
    contents.push_str(&signer_block);
    std::fs::write(path, contents).expect("write signer program");
}

use greenfloor_engine::config::parse_program_config;
use serde_json::{json, Value};

fn base_program_raw() -> Value {
    json!({
        "app": {"network": "mainnet", "home_dir": "/tmp/greenfloor-test-home", "log_level": "INFO"},
        "keys": {
            "registry": [{
                "key_id": "key-main-1",
                "fingerprint": 123456789,
                "network": "mainnet",
                "keyring_yaml_path": "~/.chia_keys/keyring.yaml"
            }]
        },
        "runtime": {"loop_interval_seconds": 30, "dry_run": false},
        "chain_signals": {
            "tx_block_trigger": {
                "mode": "websocket",
                "websocket_url": "",
                "websocket_reconnect_interval_seconds": 30,
                "fallback_poll_interval_seconds": 60
            }
        },
        "venues": {
            "dexie": {"api_base": "https://api.dexie.space"},
            "splash": {"api_base": "http://localhost:4000"},
            "offer_publish": {"provider": "dexie"}
        },
        "coin_ops": {"minimum_fee_mojos": 0},
        "dev": {"python": {"min_version": "3.11"}},
        "notifications": {
            "low_inventory_alerts": {
                "enabled": true,
                "threshold_mode": "absolute_base_units",
                "default_threshold_base_units": 0,
                "dedup_cooldown_seconds": 21600,
                "clear_hysteresis_percent": 10
            },
            "providers": [{
                "type": "pushover",
                "enabled": true,
                "user_key_env": "PUSHOVER_USER_KEY",
                "app_token_env": "PUSHOVER_APP_TOKEN",
                "recipient_key_env": "PUSHOVER_RECIPIENT_KEY"
            }]
        }
    })
}

fn parse_err(raw: &Value) -> String {
    parse_program_config(raw).unwrap_err().to_string()
}

#[test]
fn parse_program_config_minimal_valid() {
    let cfg = parse_program_config(&base_program_raw()).expect("config");
    assert_eq!(cfg.network, "mainnet");
    assert_eq!(
        cfg.home_dir,
        std::path::PathBuf::from("/tmp/greenfloor-test-home")
    );
    assert_eq!(cfg.runtime_loop_interval_seconds, 30);
    assert!(!cfg.runtime_dry_run);
    assert_eq!(cfg.tx_block_trigger_mode, "websocket");
    assert_eq!(cfg.runtime_market_slot_count, 0);
    assert!(!cfg.runtime_offer_parallelism_enabled);
    assert_eq!(cfg.runtime_offer_parallelism_max_workers, 4);
    assert_eq!(cfg.runtime_reservation_ttl_seconds, 300);
    assert_eq!(cfg.offer_publish_venue, "dexie");
    assert_eq!(cfg.coin_ops_minimum_fee_mojos, 0);
    assert_eq!(cfg.app_log_level, "INFO");
    assert!(!cfg.app_log_level_was_missing);
    assert!(cfg.signer_key_registry.contains_key("key-main-1"));
    let reg = cfg
        .signer_key_registry
        .get("key-main-1")
        .expect("registry entry");
    assert_eq!(reg.fingerprint, 123456789);
    assert_eq!(reg.network.as_deref(), Some("mainnet"));
}

#[test]
fn parse_program_config_websocket_url_defaults_mainnet() {
    let mut raw = base_program_raw();
    raw["chain_signals"]["tx_block_trigger"]["websocket_url"] = json!("");
    let cfg = parse_program_config(&raw).expect("config");
    assert_eq!(cfg.tx_block_websocket_url, "wss://api.coinset.org/ws");
}

#[test]
fn parse_program_config_websocket_url_defaults_testnet11() {
    let mut raw = base_program_raw();
    raw["app"]["network"] = json!("testnet11");
    raw["chain_signals"]["tx_block_trigger"]["websocket_url"] = json!("");
    let cfg = parse_program_config(&raw).expect("config");
    assert_eq!(
        cfg.tx_block_websocket_url,
        "wss://testnet11.api.coinset.org/ws"
    );
}

#[test]
fn parse_program_config_explicit_websocket_url_preserved() {
    let mut raw = base_program_raw();
    raw["chain_signals"]["tx_block_trigger"]["websocket_url"] =
        json!("wss://custom.example.com/ws");
    let cfg = parse_program_config(&raw).expect("config");
    assert_eq!(cfg.tx_block_websocket_url, "wss://custom.example.com/ws");
}

#[test]
fn parse_program_config_rejects_cloud_wallet_block() {
    let mut raw = base_program_raw();
    raw["cloud_wallet"] = json!({
        "base_url": "https://api.vault.chia.net",
        "user_key_id": "uk-123",
        "private_key_pem_path": "/tmp/key.pem",
        "vault_id": "Wallet_abc"
    });
    let err = parse_err(&raw);
    assert!(err.contains("cloud_wallet config is removed"));
}

#[test]
fn parse_program_config_rejects_cloud_wallet_mapping() {
    let mut raw = base_program_raw();
    raw["cloud_wallet"] = json!({"base_url": "https://example.com"});
    let err = parse_err(&raw);
    assert!(err.contains("cloud_wallet config is removed"));
}

#[test]
fn parse_program_config_rejects_cloud_wallet_string() {
    let mut raw = base_program_raw();
    raw["cloud_wallet"] = json!("bad");
    let err = parse_err(&raw);
    assert!(err.contains("cloud_wallet config is removed"));
}

#[test]
fn parse_program_config_runtime_offer_fields_default_without_legacy_keys() {
    let cfg = parse_program_config(&base_program_raw()).expect("config");
    assert_eq!(cfg.runtime_offer_bootstrap_wait_timeout_seconds, 120);
}

#[test]
fn parse_program_config_log_level_missing_defaults_to_info() {
    let mut raw = base_program_raw();
    raw["app"].as_object_mut().unwrap().remove("log_level");
    let cfg = parse_program_config(&raw).expect("config");
    assert_eq!(cfg.app_log_level, "INFO");
    assert!(cfg.app_log_level_was_missing);
}

#[test]
fn parse_program_config_log_level_invalid_defaults_to_info() {
    let mut raw = base_program_raw();
    raw["app"]["log_level"] = json!("VERBOSE");
    let cfg = parse_program_config(&raw).expect("config");
    assert_eq!(cfg.app_log_level, "INFO");
}

#[test]
fn parse_program_config_splash_venue() {
    let mut raw = base_program_raw();
    raw["venues"]["offer_publish"]["provider"] = json!("splash");
    let cfg = parse_program_config(&raw).expect("config");
    assert_eq!(cfg.offer_publish_venue, "splash");
}

#[test]
fn parse_program_config_market_slot_count() {
    let mut raw = base_program_raw();
    raw["runtime"]["market_slot_count"] = json!(4);
    let cfg = parse_program_config(&raw).expect("config");
    assert_eq!(cfg.runtime_market_slot_count, 4);
}

#[test]
fn parse_program_config_offer_parallelism_runtime_flags() {
    let mut raw = base_program_raw();
    raw["runtime"]["offer_parallelism_enabled"] = json!(true);
    raw["runtime"]["offer_parallelism_max_workers"] = json!(2);
    raw["runtime"]["reservation_ttl_seconds"] = json!(900);
    let cfg = parse_program_config(&raw).expect("config");
    assert!(cfg.runtime_offer_parallelism_enabled);
    assert_eq!(cfg.runtime_offer_parallelism_max_workers, 2);
    assert_eq!(cfg.runtime_reservation_ttl_seconds, 900);
}

#[test]
fn parse_program_config_multiple_keys_in_registry() {
    let mut raw = base_program_raw();
    raw["keys"]["registry"]
        .as_array_mut()
        .unwrap()
        .push(json!({"key_id": "key-main-2", "fingerprint": 987654321, "network": "mainnet"}));
    let cfg = parse_program_config(&raw).expect("config");
    assert_eq!(cfg.signer_key_registry.len(), 2);
    assert_eq!(cfg.signer_key_registry["key-main-2"].fingerprint, 987654321);
}

#[test]
fn parse_program_config_empty_registry() {
    let mut raw = base_program_raw();
    raw["keys"]["registry"] = json!([]);
    let cfg = parse_program_config(&raw).expect("config");
    assert!(cfg.signer_key_registry.is_empty());
}

#[test]
fn parse_program_config_missing_app() {
    let mut raw = base_program_raw();
    raw.as_object_mut().unwrap().remove("app");
    let err = parse_err(&raw);
    assert!(err.contains("Missing required field: app"));
}

#[test]
fn parse_program_config_missing_runtime() {
    let mut raw = base_program_raw();
    raw.as_object_mut().unwrap().remove("runtime");
    let err = parse_err(&raw);
    assert!(err.contains("Missing required field: runtime"));
}

#[test]
fn parse_program_config_missing_pushover_provider() {
    let mut raw = base_program_raw();
    raw["notifications"]["providers"] = json!([{"type": "slack"}]);
    let err = parse_err(&raw);
    assert!(err.contains("Missing notifications.providers entry with type=pushover"));
}

#[test]
fn parse_program_config_invalid_venue_provider() {
    let mut raw = base_program_raw();
    raw["venues"]["offer_publish"]["provider"] = json!("binance");
    let err = parse_err(&raw);
    assert!(err.contains("venues.offer_publish.provider must be one of"));
}

#[test]
fn parse_program_config_negative_minimum_fee_mojos() {
    let mut raw = base_program_raw();
    raw["coin_ops"]["minimum_fee_mojos"] = json!(-1);
    let err = parse_err(&raw);
    assert!(err.contains("coin_ops.minimum_fee_mojos must be >= 0"));
}

#[test]
fn parse_program_config_invalid_trigger_mode() {
    let mut raw = base_program_raw();
    raw["chain_signals"]["tx_block_trigger"]["mode"] = json!("poll");
    let err = parse_err(&raw);
    assert!(err.contains("mode must be websocket"));
}

#[test]
fn parse_program_config_reconnect_interval_too_low() {
    let mut raw = base_program_raw();
    raw["chain_signals"]["tx_block_trigger"]["websocket_reconnect_interval_seconds"] = json!(0);
    let err = parse_err(&raw);
    assert!(err.contains("websocket_reconnect_interval_seconds must be >= 1"));
}

#[test]
fn parse_program_config_fallback_poll_interval_negative() {
    let mut raw = base_program_raw();
    raw["chain_signals"]["tx_block_trigger"]["fallback_poll_interval_seconds"] = json!(-5);
    let err = parse_err(&raw);
    assert!(err.contains("fallback_poll_interval_seconds must be >= 0"));
}

#[test]
fn parse_program_config_registry_not_a_list() {
    let mut raw = base_program_raw();
    raw["keys"]["registry"] = json!("not-a-list");
    let err = parse_err(&raw);
    assert!(err.contains("keys.registry must be a list"));
}

#[test]
fn parse_program_config_registry_entry_not_a_dict() {
    let mut raw = base_program_raw();
    raw["keys"]["registry"] = json!(["not-a-dict"]);
    let err = parse_err(&raw);
    assert!(err.contains("keys.registry entries must be mappings"));
}

#[test]
fn parse_program_config_registry_empty_key_id() {
    let mut raw = base_program_raw();
    raw["keys"]["registry"] = json!([{"key_id": "", "fingerprint": 100}]);
    let err = parse_err(&raw);
    assert!(err.contains("key_id must be non-empty"));
}

#[test]
fn parse_program_config_registry_invalid_fingerprint() {
    let mut raw = base_program_raw();
    raw["keys"]["registry"] = json!([{"key_id": "k1", "fingerprint": "abc"}]);
    let err = parse_err(&raw);
    assert!(err.contains("invalid fingerprint"));
}

#[test]
fn parse_program_config_registry_non_positive_fingerprint() {
    let mut raw = base_program_raw();
    raw["keys"]["registry"] = json!([{"key_id": "k1", "fingerprint": 0}]);
    let err = parse_err(&raw);
    assert!(err.contains("fingerprint for key_id=k1 must be positive"));
}

#[test]
fn parse_program_config_registry_duplicate_key_id() {
    let mut raw = base_program_raw();
    raw["keys"]["registry"] = json!([
        {"key_id": "k1", "fingerprint": 100},
        {"key_id": "k1", "fingerprint": 200}
    ]);
    let err = parse_err(&raw);
    assert!(err.contains("duplicate key_id"));
}

#[test]
fn parse_program_config_registry_none_treated_as_empty() {
    let mut raw = base_program_raw();
    raw["keys"]["registry"] = json!(null);
    let cfg = parse_program_config(&raw).expect("config");
    assert!(cfg.signer_key_registry.is_empty());
}

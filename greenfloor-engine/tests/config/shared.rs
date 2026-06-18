use serde_json::{json, Value};

pub fn base_program_raw() -> Value {
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

pub fn base_market_row() -> Value {
    json!({
        "id": "m1",
        "enabled": true,
        "base_asset": "asset1",
        "base_symbol": "AS1",
        "quote_asset": "xch",
        "quote_asset_type": "unstable",
        "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        "mode": "sell_only",
        "signer_key_id": "key-main-1",
        "inventory": {"low_watermark_base_units": 1},
        "ladders": {"sell": [{"size_base_units": 1, "target_count": 1}]},
        "pricing": {},
    })
}

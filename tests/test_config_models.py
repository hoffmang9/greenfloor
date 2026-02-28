from __future__ import annotations

import pytest

from greenfloor.config.models import parse_markets_config, parse_program_config


def _base_market_row() -> dict:
    return {
        "id": "m1",
        "enabled": True,
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
    }


def _base_program_raw() -> dict:
    """Minimal valid program config raw dict."""
    return {
        "app": {"network": "mainnet", "home_dir": "~/.greenfloor", "log_level": "INFO"},
        "keys": {
            "registry": [
                {
                    "key_id": "key-main-1",
                    "fingerprint": 123456789,
                    "network": "mainnet",
                    "keyring_yaml_path": "~/.chia_keys/keyring.yaml",
                }
            ]
        },
        "runtime": {"loop_interval_seconds": 30, "dry_run": False},
        "chain_signals": {
            "tx_block_trigger": {
                "mode": "websocket",
                "websocket_url": "",
                "websocket_reconnect_interval_seconds": 30,
                "fallback_poll_interval_seconds": 60,
            }
        },
        "venues": {
            "dexie": {"api_base": "https://api.dexie.space"},
            "splash": {"api_base": "http://localhost:4000"},
            "offer_publish": {"provider": "dexie"},
        },
        "coin_ops": {"minimum_fee_mojos": 0},
        "dev": {"python": {"min_version": "3.11"}},
        "notifications": {
            "low_inventory_alerts": {
                "enabled": True,
                "threshold_mode": "absolute_base_units",
                "default_threshold_base_units": 0,
                "dedup_cooldown_seconds": 21600,
                "clear_hysteresis_percent": 10,
            },
            "providers": [
                {
                    "type": "pushover",
                    "enabled": True,
                    "user_key_env": "PUSHOVER_USER_KEY",
                    "app_token_env": "PUSHOVER_APP_TOKEN",
                    "recipient_key_env": "PUSHOVER_RECIPIENT_KEY",
                }
            ],
        },
    }


# ---------------------------------------------------------------------------
# parse_markets_config tests
# ---------------------------------------------------------------------------


def test_parse_markets_config_rejects_invalid_strategy_spread() -> None:
    row = _base_market_row()
    row["pricing"] = {"strategy_target_spread_bps": 0}
    with pytest.raises(ValueError, match="strategy_target_spread_bps"):
        parse_markets_config({"markets": [row]})


def test_parse_markets_config_rejects_invalid_strategy_price_band() -> None:
    row = _base_market_row()
    row["pricing"] = {
        "strategy_min_xch_price_usd": 50.0,
        "strategy_max_xch_price_usd": 40.0,
    }
    with pytest.raises(
        ValueError, match="strategy_min_xch_price_usd must be <= strategy_max_xch_price_usd"
    ):
        parse_markets_config({"markets": [row]})


def test_parse_markets_config_accepts_valid_strategy_controls() -> None:
    row = _base_market_row()
    row["pricing"] = {
        "strategy_target_spread_bps": 120,
        "strategy_min_xch_price_usd": 20.0,
        "strategy_max_xch_price_usd": 60.0,
    }
    out = parse_markets_config({"markets": [row]})
    assert len(out.markets) == 1
    assert out.markets[0].pricing["strategy_target_spread_bps"] == 120


def test_parse_markets_config_rejects_partial_strategy_expiry_override() -> None:
    row = _base_market_row()
    row["pricing"] = {"strategy_offer_expiry_unit": "hours"}
    with pytest.raises(
        ValueError,
        match="strategy_offer_expiry_unit and strategy_offer_expiry_value must be set together",
    ):
        parse_markets_config({"markets": [row]})


def test_parse_markets_config_rejects_invalid_strategy_expiry_unit() -> None:
    row = _base_market_row()
    row["pricing"] = {
        "strategy_offer_expiry_unit": "days",
        "strategy_offer_expiry_value": 1,
    }
    with pytest.raises(ValueError, match="strategy_offer_expiry_unit must be one of"):
        parse_markets_config({"markets": [row]})


def test_parse_markets_config_accepts_strategy_expiry_override() -> None:
    row = _base_market_row()
    row["pricing"] = {
        "strategy_offer_expiry_unit": "hours",
        "strategy_offer_expiry_value": 2,
    }
    out = parse_markets_config({"markets": [row]})
    assert out.markets[0].pricing["strategy_offer_expiry_unit"] == "hours"
    assert out.markets[0].pricing["strategy_offer_expiry_value"] == 2


# ---------------------------------------------------------------------------
# parse_program_config: happy path
# ---------------------------------------------------------------------------


def test_parse_program_config_minimal_valid() -> None:
    raw = _base_program_raw()
    cfg = parse_program_config(raw)
    assert cfg.app_network == "mainnet"
    assert cfg.home_dir == "~/.greenfloor"
    assert cfg.runtime_loop_interval_seconds == 30
    assert cfg.runtime_dry_run is False
    assert cfg.tx_block_trigger_mode == "websocket"
    assert cfg.offer_publish_venue == "dexie"
    assert cfg.coin_ops_minimum_fee_mojos == 0
    assert cfg.app_log_level == "INFO"
    assert cfg.app_log_level_was_missing is False
    assert "key-main-1" in cfg.signer_key_registry
    reg = cfg.signer_key_registry["key-main-1"]
    assert reg.fingerprint == 123456789
    assert reg.network == "mainnet"


def test_parse_program_config_websocket_url_defaults_mainnet() -> None:
    raw = _base_program_raw()
    raw["chain_signals"]["tx_block_trigger"]["websocket_url"] = ""
    cfg = parse_program_config(raw)
    assert cfg.tx_block_websocket_url == "wss://api.coinset.org/ws"


def test_parse_program_config_websocket_url_defaults_testnet11() -> None:
    raw = _base_program_raw()
    raw["app"]["network"] = "testnet11"
    raw["chain_signals"]["tx_block_trigger"]["websocket_url"] = ""
    cfg = parse_program_config(raw)
    assert cfg.tx_block_websocket_url == "wss://testnet11.api.coinset.org/ws"


def test_parse_program_config_explicit_websocket_url_preserved() -> None:
    raw = _base_program_raw()
    raw["chain_signals"]["tx_block_trigger"]["websocket_url"] = "wss://custom.example.com/ws"
    cfg = parse_program_config(raw)
    assert cfg.tx_block_websocket_url == "wss://custom.example.com/ws"


def test_parse_program_config_cloud_wallet_fields() -> None:
    raw = _base_program_raw()
    raw["cloud_wallet"] = {
        "base_url": "https://api.vault.chia.net",
        "user_key_id": "uk-123",
        "private_key_pem_path": "/tmp/key.pem",
        "vault_id": "Wallet_abc",
    }
    cfg = parse_program_config(raw)
    assert cfg.cloud_wallet_base_url == "https://api.vault.chia.net"
    assert cfg.cloud_wallet_user_key_id == "uk-123"
    assert cfg.cloud_wallet_private_key_pem_path == "/tmp/key.pem"
    assert cfg.cloud_wallet_vault_id == "Wallet_abc"


def test_parse_program_config_cloud_wallet_defaults_empty() -> None:
    raw = _base_program_raw()
    cfg = parse_program_config(raw)
    assert cfg.cloud_wallet_base_url == ""
    assert cfg.cloud_wallet_vault_id == ""


def test_parse_program_config_log_level_missing_defaults_to_info() -> None:
    raw = _base_program_raw()
    del raw["app"]["log_level"]
    cfg = parse_program_config(raw)
    assert cfg.app_log_level == "INFO"
    assert cfg.app_log_level_was_missing is True


def test_parse_program_config_log_level_invalid_defaults_to_info() -> None:
    raw = _base_program_raw()
    raw["app"]["log_level"] = "VERBOSE"
    cfg = parse_program_config(raw)
    assert cfg.app_log_level == "INFO"


def test_parse_program_config_splash_venue() -> None:
    raw = _base_program_raw()
    raw["venues"]["offer_publish"]["provider"] = "splash"
    cfg = parse_program_config(raw)
    assert cfg.offer_publish_venue == "splash"


def test_parse_program_config_multiple_keys_in_registry() -> None:
    raw = _base_program_raw()
    raw["keys"]["registry"].append(
        {"key_id": "key-main-2", "fingerprint": 987654321, "network": "mainnet"}
    )
    cfg = parse_program_config(raw)
    assert len(cfg.signer_key_registry) == 2
    assert cfg.signer_key_registry["key-main-2"].fingerprint == 987654321


def test_parse_program_config_empty_registry() -> None:
    raw = _base_program_raw()
    raw["keys"]["registry"] = []
    cfg = parse_program_config(raw)
    assert cfg.signer_key_registry == {}


# ---------------------------------------------------------------------------
# parse_program_config: validation failures
# ---------------------------------------------------------------------------


def test_parse_program_config_missing_app() -> None:
    raw = _base_program_raw()
    del raw["app"]
    with pytest.raises(ValueError, match="Missing required field: app"):
        parse_program_config(raw)


def test_parse_program_config_missing_runtime() -> None:
    raw = _base_program_raw()
    del raw["runtime"]
    with pytest.raises(ValueError, match="Missing required field: runtime"):
        parse_program_config(raw)


def test_parse_program_config_missing_pushover_provider() -> None:
    raw = _base_program_raw()
    raw["notifications"]["providers"] = [{"type": "slack"}]
    with pytest.raises(
        ValueError, match="Missing notifications.providers entry with type=pushover"
    ):
        parse_program_config(raw)


def test_parse_program_config_invalid_venue_provider() -> None:
    raw = _base_program_raw()
    raw["venues"]["offer_publish"]["provider"] = "binance"
    with pytest.raises(ValueError, match="venues.offer_publish.provider must be one of"):
        parse_program_config(raw)


def test_parse_program_config_negative_minimum_fee_mojos() -> None:
    raw = _base_program_raw()
    raw["coin_ops"]["minimum_fee_mojos"] = -1
    with pytest.raises(ValueError, match="coin_ops.minimum_fee_mojos must be >= 0"):
        parse_program_config(raw)


def test_parse_program_config_invalid_trigger_mode() -> None:
    raw = _base_program_raw()
    raw["chain_signals"]["tx_block_trigger"]["mode"] = "poll"
    with pytest.raises(ValueError, match="mode must be websocket"):
        parse_program_config(raw)


def test_parse_program_config_reconnect_interval_too_low() -> None:
    raw = _base_program_raw()
    raw["chain_signals"]["tx_block_trigger"]["websocket_reconnect_interval_seconds"] = 0
    with pytest.raises(ValueError, match="websocket_reconnect_interval_seconds must be >= 1"):
        parse_program_config(raw)


def test_parse_program_config_fallback_poll_interval_negative() -> None:
    raw = _base_program_raw()
    raw["chain_signals"]["tx_block_trigger"]["fallback_poll_interval_seconds"] = -5
    with pytest.raises(ValueError, match="fallback_poll_interval_seconds must be >= 0"):
        parse_program_config(raw)


def test_parse_program_config_cloud_wallet_not_a_dict() -> None:
    raw = _base_program_raw()
    raw["cloud_wallet"] = "bad"
    with pytest.raises(ValueError, match="cloud_wallet must be a mapping"):
        parse_program_config(raw)


def test_parse_program_config_cloud_wallet_none_treated_as_empty() -> None:
    raw = _base_program_raw()
    raw["cloud_wallet"] = None
    cfg = parse_program_config(raw)
    assert cfg.cloud_wallet_base_url == ""


# ---------------------------------------------------------------------------
# parse_program_config: key registry validation
# ---------------------------------------------------------------------------


def test_parse_program_config_registry_not_a_list() -> None:
    raw = _base_program_raw()
    raw["keys"]["registry"] = "not-a-list"
    with pytest.raises(ValueError, match="keys.registry must be a list"):
        parse_program_config(raw)


def test_parse_program_config_registry_entry_not_a_dict() -> None:
    raw = _base_program_raw()
    raw["keys"]["registry"] = ["not-a-dict"]
    with pytest.raises(ValueError, match="keys.registry entries must be mappings"):
        parse_program_config(raw)


def test_parse_program_config_registry_empty_key_id() -> None:
    raw = _base_program_raw()
    raw["keys"]["registry"] = [{"key_id": "", "fingerprint": 100}]
    with pytest.raises(ValueError, match="key_id must be non-empty"):
        parse_program_config(raw)


def test_parse_program_config_registry_invalid_fingerprint() -> None:
    raw = _base_program_raw()
    raw["keys"]["registry"] = [{"key_id": "k1", "fingerprint": "abc"}]
    with pytest.raises(ValueError, match="invalid fingerprint"):
        parse_program_config(raw)


def test_parse_program_config_registry_non_positive_fingerprint() -> None:
    raw = _base_program_raw()
    raw["keys"]["registry"] = [{"key_id": "k1", "fingerprint": 0}]
    with pytest.raises(ValueError, match="fingerprint for key_id=k1 must be positive"):
        parse_program_config(raw)


def test_parse_program_config_registry_duplicate_key_id() -> None:
    raw = _base_program_raw()
    raw["keys"]["registry"] = [
        {"key_id": "k1", "fingerprint": 100},
        {"key_id": "k1", "fingerprint": 200},
    ]
    with pytest.raises(ValueError, match="duplicate key_id"):
        parse_program_config(raw)


def test_parse_program_config_registry_none_treated_as_empty() -> None:
    raw = _base_program_raw()
    raw["keys"]["registry"] = None
    cfg = parse_program_config(raw)
    assert cfg.signer_key_registry == {}

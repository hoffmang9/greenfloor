"""Canonical ProgramConfig and MarketConfig fixtures for tests."""

from __future__ import annotations

from greenfloor.config.models import MarketConfig, MarketInventoryConfig, ProgramConfig


def minimal_program_config(*, home_dir: str = "/tmp/gf-test") -> ProgramConfig:
    return ProgramConfig(
        app_network="mainnet",
        home_dir=home_dir,
        runtime_loop_interval_seconds=30,
        runtime_dry_run=False,
        tx_block_trigger_mode="websocket",
        tx_block_websocket_url="wss://coinset.org/ws",
        tx_block_websocket_reconnect_interval_seconds=30,
        tx_block_fallback_poll_interval_seconds=60,
        tx_block_webhook_enabled=True,
        tx_block_webhook_listen_addr="127.0.0.1:8787",
        dexie_api_base="https://api.dexie.space",
        splash_api_base="http://localhost:4000",
        offer_publish_venue="dexie",
        coin_ops_max_operations_per_run=20,
        coin_ops_max_daily_fee_budget_mojos=0,
        coin_ops_minimum_fee_mojos=0,
        coin_ops_split_fee_mojos=0,
        coin_ops_combine_fee_mojos=0,
        python_min_version="3.11",
        low_inventory_enabled=False,
        low_inventory_threshold_mode="absolute_base_units",
        low_inventory_default_threshold_base_units=0,
        low_inventory_dedup_cooldown_seconds=3600,
        low_inventory_clear_hysteresis_percent=10,
        pushover_enabled=False,
        pushover_user_key_env="PUSHOVER_USER_KEY",
        pushover_app_token_env="PUSHOVER_APP_TOKEN",
        pushover_recipient_key_env="PUSHOVER_RECIPIENT_KEY",
    )


def minimal_market_config(*, market_id: str = "m1") -> MarketConfig:
    return MarketConfig(
        market_id=market_id,
        enabled=True,
        base_asset="a1",
        base_symbol="A1",
        quote_asset="xch",
        quote_asset_type="unstable",
        receive_address="xch1test",
        mode="sell_only",
        signer_key_id="k1",
        inventory=MarketInventoryConfig(low_watermark_base_units=10),
    )

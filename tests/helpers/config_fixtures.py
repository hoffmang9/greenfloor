"""Canonical ProgramConfig and MarketConfig fixtures for tests."""

from __future__ import annotations

from dataclasses import replace

from greenfloor.config.models import (
    MarketConfig,
    MarketInventoryConfig,
    MarketLadderEntry,
    ProgramConfig,
)


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


def minimal_market_with_sell_ladder(
    *,
    market_id: str = "m1",
    size_base_units: int = 1,
    target_count: int = 2,
    **overrides: object,
) -> MarketConfig:
    return replace(
        minimal_market_config(market_id=market_id),
        ladders={
            "sell": [
                MarketLadderEntry(
                    size_base_units=size_base_units,
                    target_count=target_count,
                    split_buffer_count=0,
                    combine_when_excess_factor=2.0,
                )
            ]
        },
        **overrides,
    )


def minimal_market_with_tiered_sell_ladder(
    *,
    market_id: str = "m1",
    **overrides: object,
) -> MarketConfig:
    """Sell ladder with 1 / 10 / 100 base-unit tiers (matches bootstrap planner tests)."""
    return replace(
        minimal_market_config(market_id=market_id),
        ladders={
            "sell": [
                MarketLadderEntry(
                    size_base_units=1,
                    target_count=3,
                    split_buffer_count=0,
                    combine_when_excess_factor=2.0,
                ),
                MarketLadderEntry(
                    size_base_units=10,
                    target_count=2,
                    split_buffer_count=1,
                    combine_when_excess_factor=2.0,
                ),
                MarketLadderEntry(
                    size_base_units=100,
                    target_count=1,
                    split_buffer_count=0,
                    combine_when_excess_factor=2.0,
                ),
            ]
        },
        **overrides,
    )

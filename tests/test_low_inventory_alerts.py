from datetime import UTC, datetime, timedelta

from greenfloor.config.models import MarketConfig, MarketInventoryConfig, ProgramConfig
from greenfloor.core.notifications import AlertState, evaluate_low_inventory_alert


def _program() -> ProgramConfig:
    return ProgramConfig(
        app_network="mainnet",
        home_dir="~/.greenfloor",
        runtime_loop_interval_seconds=30,
        runtime_dry_run=True,
        tx_block_webhook_enabled=True,
        tx_block_webhook_listen_addr="127.0.0.1:8787",
        dexie_api_base="https://api.dexie.space",
        splash_api_base="http://localhost:4000",
        offer_publish_venue="dexie",
        coin_ops_max_operations_per_run=20,
        coin_ops_max_daily_fee_budget_mojos=0,
        coin_ops_split_fee_mojos=0,
        coin_ops_combine_fee_mojos=0,
        python_min_version="3.11",
        low_inventory_enabled=True,
        low_inventory_threshold_mode="absolute_base_units",
        low_inventory_default_threshold_base_units=0,
        low_inventory_dedup_cooldown_seconds=3600,
        low_inventory_clear_hysteresis_percent=10,
        pushover_enabled=False,
        pushover_user_key_env="PUSHOVER_USER_KEY",
        pushover_app_token_env="PUSHOVER_APP_TOKEN",
        pushover_recipient_key_env="PUSHOVER_RECIPIENT_KEY",
    )


def _market(remaining: int) -> MarketConfig:
    return MarketConfig(
        market_id="carbon_xch_sell",
        enabled=True,
        base_asset="asset",
        base_symbol="BYC",
        quote_asset="xch",
        quote_asset_type="unstable",
        receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        mode="sell_only",
        signer_key_id="key-a",
        inventory=MarketInventoryConfig(
            low_watermark_base_units=100,
            low_inventory_alert_threshold_base_units=None,
            current_available_base_units=remaining,
        ),
    )


def test_low_inventory_triggers_first_alert() -> None:
    now = datetime.now(UTC)
    state, event = evaluate_low_inventory_alert(
        now=now,
        program=_program(),
        market=_market(90),
        state=AlertState(),
    )
    assert state.is_low is True
    assert event is not None
    assert event.ticker == "BYC"
    assert event.remaining_amount == 90


def test_low_inventory_dedup_respects_cooldown() -> None:
    now = datetime.now(UTC)
    prior = AlertState(is_low=True, last_alert_at=now - timedelta(minutes=30))
    state, event = evaluate_low_inventory_alert(
        now=now,
        program=_program(),
        market=_market(80),
        state=prior,
    )
    assert state.is_low is True
    assert event is None


def test_low_inventory_clears_with_hysteresis() -> None:
    now = datetime.now(UTC)
    prior = AlertState(is_low=True, last_alert_at=now - timedelta(hours=2))
    # threshold=100; clear target=110
    state, event = evaluate_low_inventory_alert(
        now=now,
        program=_program(),
        market=_market(111),
        state=prior,
    )
    assert state.is_low is False
    assert event is None

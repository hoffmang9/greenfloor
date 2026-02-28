"""Tests for daemon/main.py small helper functions: env, retry, cooldown, pricing."""

from __future__ import annotations

import time
from typing import Any

from greenfloor.daemon import main as daemon_main
from greenfloor.daemon.main import (
    _abs_move_bps,
    _cancel_retry_config,
    _cloud_wallet_configured,
    _cooldown_remaining_ms,
    _disabled_market_log_interval_seconds,
    _env_int,
    _log_disabled_markets_startup_once,
    _market_pricing,
    _post_retry_config,
    _resolve_quote_asset_for_offer,
    _retry_with_backoff,
    _set_cooldown,
    _should_log_disabled_market,
)

# ---------------------------------------------------------------------------
# _env_int
# ---------------------------------------------------------------------------


def test_env_int_returns_default_when_unset(monkeypatch) -> None:
    monkeypatch.delenv("GF_TEST_INT", raising=False)
    assert _env_int("GF_TEST_INT", 42) == 42


def test_env_int_returns_parsed_value(monkeypatch) -> None:
    monkeypatch.setenv("GF_TEST_INT", "99")
    assert _env_int("GF_TEST_INT", 0) == 99


def test_env_int_applies_minimum(monkeypatch) -> None:
    monkeypatch.setenv("GF_TEST_INT", "0")
    assert _env_int("GF_TEST_INT", 5, minimum=1) == 1


def test_env_int_returns_default_for_invalid(monkeypatch) -> None:
    monkeypatch.setenv("GF_TEST_INT", "abc")
    assert _env_int("GF_TEST_INT", 7) == 7


# ---------------------------------------------------------------------------
# _cooldown_remaining_ms / _set_cooldown
# ---------------------------------------------------------------------------


def test_cooldown_remaining_ms_zero_when_not_set() -> None:
    assert _cooldown_remaining_ms({}, "key") == 0


def test_cooldown_remaining_ms_positive_when_future() -> None:
    cooldowns: dict[str, float] = {"key": time.monotonic() + 5.0}
    remaining = _cooldown_remaining_ms(cooldowns, "key")
    assert remaining > 0
    assert remaining <= 5000


def test_set_cooldown_creates_deadline() -> None:
    cooldowns: dict[str, float] = {}
    _set_cooldown(cooldowns, "key", 10)
    assert "key" in cooldowns
    assert cooldowns["key"] > time.monotonic()


def test_set_cooldown_ignores_non_positive() -> None:
    cooldowns: dict[str, float] = {}
    _set_cooldown(cooldowns, "key", 0)
    assert "key" not in cooldowns
    _set_cooldown(cooldowns, "key", -1)
    assert "key" not in cooldowns


# ---------------------------------------------------------------------------
# _retry_with_backoff
# ---------------------------------------------------------------------------


def test_retry_with_backoff_succeeds_first_try() -> None:
    result, attempts, error = _retry_with_backoff(
        action_fn=lambda: {"success": True, "id": "x"},
        is_success=lambda r: bool(r.get("success")),
        default_error="fail",
        retry_config=(3, 0, 0),
    )
    assert result["success"] is True
    assert attempts == 1
    assert error == ""


def test_retry_with_backoff_retries_then_succeeds() -> None:
    call_count = {"n": 0}

    def _action() -> dict[str, Any]:
        call_count["n"] += 1
        if call_count["n"] < 3:
            return {"success": False, "error": "transient"}
        return {"success": True}

    result, attempts, error = _retry_with_backoff(
        action_fn=_action,
        is_success=lambda r: bool(r.get("success")),
        default_error="fail",
        retry_config=(5, 0, 0),
    )
    assert result["success"] is True
    assert attempts == 3
    assert error == ""


def test_retry_with_backoff_exhausts_attempts() -> None:
    result, attempts, error = _retry_with_backoff(
        action_fn=lambda: {"success": False, "error": "permanent"},
        is_success=lambda r: bool(r.get("success")),
        default_error="fail",
        retry_config=(2, 0, 0),
    )
    assert result["success"] is False
    assert attempts == 2
    assert error == "permanent"


def test_retry_with_backoff_handles_exception() -> None:
    def _action() -> dict[str, Any]:
        raise RuntimeError("boom")

    result, attempts, error = _retry_with_backoff(
        action_fn=_action,
        is_success=lambda r: bool(r.get("success")),
        default_error="op_failed",
        retry_config=(1, 0, 0),
    )
    assert result["success"] is False
    assert "boom" in error


# ---------------------------------------------------------------------------
# _post_retry_config / _cancel_retry_config
# ---------------------------------------------------------------------------


def test_post_retry_config_defaults(monkeypatch) -> None:
    monkeypatch.delenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", raising=False)
    monkeypatch.delenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", raising=False)
    monkeypatch.delenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", raising=False)
    attempts, backoff_ms, cooldown = _post_retry_config()
    assert attempts == 2
    assert backoff_ms == 250
    assert cooldown == 30


def test_cancel_retry_config_respects_env(monkeypatch) -> None:
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_MAX_ATTEMPTS", "5")
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", "100")
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_COOLDOWN_SECONDS", "60")
    attempts, backoff_ms, cooldown = _cancel_retry_config()
    assert attempts == 5
    assert backoff_ms == 100
    assert cooldown == 60


def test_disabled_market_log_interval_defaults(monkeypatch) -> None:
    monkeypatch.delenv("GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS", raising=False)
    assert _disabled_market_log_interval_seconds() == 3600


def test_disabled_market_log_interval_applies_minimum(monkeypatch) -> None:
    monkeypatch.setenv("GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS", "1")
    assert _disabled_market_log_interval_seconds() == 60


def test_should_log_disabled_market_throttles(monkeypatch) -> None:
    monkeypatch.setenv("GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS", "3600")
    daemon_main._DISABLED_MARKET_NEXT_LOG_AT.clear()
    assert _should_log_disabled_market(market_id="m-disabled", now_monotonic=100.0) is True
    assert _should_log_disabled_market(market_id="m-disabled", now_monotonic=200.0) is False
    assert _should_log_disabled_market(market_id="m-disabled", now_monotonic=3701.0) is True


def test_log_disabled_markets_startup_once_logs_and_seeds_throttle(monkeypatch) -> None:
    monkeypatch.setenv("GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS", "3600")
    daemon_main._DISABLED_MARKET_NEXT_LOG_AT.clear()
    daemon_main._DISABLED_MARKET_STARTUP_LOGGED = False

    class _Market:
        def __init__(self, market_id: str, enabled: bool) -> None:
            self.market_id = market_id
            self.enabled = enabled

    logged: list[tuple[Any, ...]] = []
    monkeypatch.setattr(daemon_main._daemon_logger, "info", lambda *args: logged.append(args))

    _log_disabled_markets_startup_once(
        markets=[_Market("enabled-market", True), _Market("disabled-market", False)]
    )
    _log_disabled_markets_startup_once(
        markets=[_Market("enabled-market", True), _Market("disabled-market", False)]
    )

    assert len(logged) == 1
    assert "disabled_markets_startup" in str(logged[0][0])
    assert "disabled-market" in daemon_main._DISABLED_MARKET_NEXT_LOG_AT
    daemon_main._DISABLED_MARKET_STARTUP_LOGGED = False


# ---------------------------------------------------------------------------
# _abs_move_bps
# ---------------------------------------------------------------------------


def test_abs_move_bps_positive_move() -> None:
    result = _abs_move_bps(110.0, 100.0)
    assert result is not None
    assert abs(result - 1000.0) < 0.01


def test_abs_move_bps_returns_none_for_none_inputs() -> None:
    assert _abs_move_bps(None, 100.0) is None
    assert _abs_move_bps(100.0, None) is None


def test_abs_move_bps_returns_none_for_non_positive() -> None:
    assert _abs_move_bps(0.0, 100.0) is None
    assert _abs_move_bps(100.0, 0.0) is None


# ---------------------------------------------------------------------------
# _cloud_wallet_configured
# ---------------------------------------------------------------------------


def test_cloud_wallet_configured_true() -> None:
    class _Prog:
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "key-1"
        cloud_wallet_private_key_pem_path = "/tmp/key.pem"
        cloud_wallet_vault_id = "Wallet_abc"

    assert _cloud_wallet_configured(_Prog()) is True


def test_cloud_wallet_configured_false_when_empty() -> None:
    class _Prog:
        cloud_wallet_base_url = ""
        cloud_wallet_user_key_id = "key-1"
        cloud_wallet_private_key_pem_path = "/tmp/key.pem"
        cloud_wallet_vault_id = "Wallet_abc"

    assert _cloud_wallet_configured(_Prog()) is False


# ---------------------------------------------------------------------------
# _market_pricing
# ---------------------------------------------------------------------------


def test_market_pricing_extracts_dict() -> None:
    class _Market:
        pricing = {"spread_bps": 100}

    assert _market_pricing(_Market()) == {"spread_bps": 100}


def test_market_pricing_handles_none() -> None:
    class _Market:
        pricing = None

    assert _market_pricing(_Market()) == {}


def test_market_pricing_handles_missing_attr() -> None:
    class _Market:
        pass

    assert _market_pricing(_Market()) == {}


# ---------------------------------------------------------------------------
# _resolve_quote_asset_for_offer
# ---------------------------------------------------------------------------


def test_resolve_quote_asset_xch_mainnet() -> None:
    assert _resolve_quote_asset_for_offer(quote_asset="xch", network="mainnet") == "xch"


def test_resolve_quote_asset_xch_testnet() -> None:
    assert _resolve_quote_asset_for_offer(quote_asset="xch", network="testnet11") == "txch"


def test_resolve_quote_asset_hex_passthrough() -> None:
    hex_id = "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d"
    assert _resolve_quote_asset_for_offer(quote_asset=hex_id, network="mainnet") == hex_id

"""Tests for daemon helper functions: env, retry, cooldown, pricing."""

from __future__ import annotations

import time
from datetime import UTC, datetime
from typing import Any

from greenfloor.daemon import cycle_market_batch
from greenfloor.daemon.cooldowns import _env_int
from greenfloor.daemon.cycle_market_batch import (
    _DISABLED_MARKET_NEXT_LOG_AT,
)
from greenfloor.daemon.cycle_market_batch import (
    disabled_market_log_interval_seconds as _disabled_market_log_interval_seconds,
)
from greenfloor.daemon.cycle_market_batch import (
    log_disabled_markets_startup_once as _log_disabled_markets_startup_once,
)
from greenfloor.daemon.cycle_market_batch import (
    should_log_disabled_market as _should_log_disabled_market,
)
from greenfloor.daemon.market_helpers import (
    _abs_move_bps,
    _market_pricing,
    _resolve_quote_asset_for_offer,
)
from greenfloor.daemon.market_logging import _daemon_logger
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.daemon.testing import (
    cancel_retry_config,
    cooldown_remaining_ms,
    cooldowns,
    post_retry_config,
    set_cooldown,
)
from greenfloor.runtime.coin_ops.coins import coin_matches_direct_spendable_lookup

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
# cooldown_remaining_ms / set_cooldown
# ---------------------------------------------------------------------------


def test_cooldown_remaining_ms_zero_when_not_set() -> None:
    assert cooldown_remaining_ms({}, "key") == 0


def test_cooldown_remaining_ms_positive_when_future() -> None:
    cooldowns: dict[str, float] = {"key": time.monotonic() + 5.0}
    remaining = cooldown_remaining_ms(cooldowns, "key")
    assert remaining > 0
    assert remaining <= 5000


def testset_cooldown_creates_deadline() -> None:
    cooldowns: dict[str, float] = {}
    set_cooldown(cooldowns, "key", 10)
    assert "key" in cooldowns
    assert cooldowns["key"] > time.monotonic()


def testset_cooldown_ignores_non_positive() -> None:
    cooldowns: dict[str, float] = {}
    set_cooldown(cooldowns, "key", 0)
    assert "key" not in cooldowns
    set_cooldown(cooldowns, "key", -1)
    assert "key" not in cooldowns


# ---------------------------------------------------------------------------
# retry_with_backoff
# ---------------------------------------------------------------------------


def test_retry_with_backoff_succeeds_first_try() -> None:
    result, attempts, error = cooldowns._retry_with_backoff(
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

    result, attempts, error = cooldowns._retry_with_backoff(
        action_fn=_action,
        is_success=lambda r: bool(r.get("success")),
        default_error="fail",
        retry_config=(5, 0, 0),
    )
    assert result["success"] is True
    assert attempts == 3
    assert error == ""


def test_retry_with_backoff_exhausts_attempts() -> None:
    result, attempts, error = cooldowns._retry_with_backoff(
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

    result, attempts, error = cooldowns._retry_with_backoff(
        action_fn=_action,
        is_success=lambda r: bool(r.get("success")),
        default_error="op_failed",
        retry_config=(1, 0, 0),
    )
    assert result["success"] is False
    assert "boom" in error


# ---------------------------------------------------------------------------
# post_retry_config / cancel_retry_config
# ---------------------------------------------------------------------------


def testpost_retry_config_defaults(monkeypatch) -> None:
    monkeypatch.delenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", raising=False)
    monkeypatch.delenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", raising=False)
    monkeypatch.delenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", raising=False)
    attempts, backoff_ms, cooldown = post_retry_config()
    assert attempts == 2
    assert backoff_ms == 250
    assert cooldown == 30


def testcancel_retry_config_respects_env(monkeypatch) -> None:
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_MAX_ATTEMPTS", "5")
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", "100")
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_COOLDOWN_SECONDS", "60")
    attempts, backoff_ms, cooldown = cancel_retry_config()
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
    _DISABLED_MARKET_NEXT_LOG_AT.clear()
    assert _should_log_disabled_market(market_id="m-disabled", now_monotonic=100.0) is True
    assert _should_log_disabled_market(market_id="m-disabled", now_monotonic=200.0) is False
    assert _should_log_disabled_market(market_id="m-disabled", now_monotonic=3701.0) is True


def test_log_disabled_markets_startup_once_logs_and_seeds_throttle(monkeypatch) -> None:
    monkeypatch.setenv("GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS", "3600")
    _DISABLED_MARKET_NEXT_LOG_AT.clear()
    cycle_market_batch._DISABLED_MARKET_STARTUP_LOGGED = False

    class _Market:
        def __init__(self, market_id: str, enabled: bool) -> None:
            self.market_id = market_id
            self.enabled = enabled

    logged: list[tuple[Any, ...]] = []
    monkeypatch.setattr(_daemon_logger, "info", lambda *args: logged.append(args))

    _log_disabled_markets_startup_once(
        markets=[_Market("enabled-market", True), _Market("disabled-market", False)]
    )
    _log_disabled_markets_startup_once(
        markets=[_Market("enabled-market", True), _Market("disabled-market", False)]
    )

    assert len(logged) == 1
    assert "disabled_markets_startup" in str(logged[0][0])
    assert "disabled-market" in _DISABLED_MARKET_NEXT_LOG_AT
    cycle_market_batch._DISABLED_MARKET_STARTUP_LOGGED = False


def test_managed_offer_market_health_payload_tracks_503_and_last_success() -> None:
    # Clear any leftover in-memory state from prior test runs.
    cooldowns._MANAGED_OFFER_HEALTH_WINDOW.pop("m1-health-test", None)

    success_time = datetime(2026, 3, 19, 2, 50, 0, tzinfo=UTC)
    cooldowns._managed_offer_market_health_payload(
        market_id="m1-health-test",
        current_items=[
            StrategyActionItem(
                size=1,
                side="sell",
                status="executed",
                reason="managed_offer_post_success",
            ),
            StrategyActionItem(
                size=1,
                side="sell",
                status="skipped",
                reason="managed_offer_action_error:managed_offer_http_error:503:<html>...</html>",
            ),
        ],
        now=success_time,
        window_size=20,
    )

    now = datetime(2026, 3, 19, 3, 0, 0, tzinfo=UTC)
    payload = cooldowns._managed_offer_market_health_payload(
        market_id="m1-health-test",
        current_items=[
            StrategyActionItem(
                size=1,
                side="sell",
                status="skipped",
                reason="managed_offer_action_error:managed_offer_http_error:503:<html>...</html>",
            )
        ],
        now=now,
        window_size=20,
    )
    assert payload["market_id"] == "m1-health-test"
    assert payload["rolling_window_events"] == 2
    assert payload["rolling_503_count"] == 2
    assert payload["last_managed_offer_success_at"] == "2026-03-19T02:50:00+00:00"
    assert payload["last_managed_offer_success_age_seconds"] == 600

    cooldowns._MANAGED_OFFER_HEALTH_WINDOW.pop("m1-health-test", None)


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


def test_direct_spendable_lookup_fails_open_on_lookup_exception() -> None:
    class _Wallet:
        @staticmethod
        def get_coin_record(*, coin_id: str) -> dict[str, Any]:
            _ = coin_id
            raise TimeoutError("The read operation timed out")

    coin = {"id": "coin-1", "state": "SETTLED", "isLocked": False}
    assert coin_matches_direct_spendable_lookup(
        wallet=_Wallet(),
        coin=coin,
        scoped_asset_id="asset-1",
        cache={},
        fail_open_on_lookup_error=True,
    )


def test_direct_spendable_lookup_accepts_missing_asset_metadata() -> None:
    class _Wallet:
        @staticmethod
        def get_coin_record(*, coin_id: str) -> dict[str, Any]:
            _ = coin_id
            return {
                "id": "coin-1",
                "state": "SETTLED",
                "isLocked": False,
                "isLinkedToOpenOffer": False,
            }

    coin = {"id": "coin-1", "state": "SETTLED", "isLocked": False}
    assert coin_matches_direct_spendable_lookup(
        wallet=_Wallet(),
        coin=coin,
        scoped_asset_id="asset-1",
        cache={},
        fail_open_on_lookup_error=True,
    )

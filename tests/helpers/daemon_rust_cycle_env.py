"""Shared pytest hooks for in-process Rust daemon cycle tests."""

from __future__ import annotations

from typing import Any

from tests.helpers.engine_binary import engine_binary_path


def new_coin_watchlist_cache() -> Any:
    from greenfloor.core.engine_bridge import import_engine, require_engine_method

    cache_cls = require_engine_method(
        import_engine(),
        "CoinWatchlistCache",
        missing="coin watchlist cache",
    )
    return cache_cls()


def run_once_for_tests(*args, test_controls=None, **kwargs) -> int:
    """Test entrypoint: default controls, per-call watchlist when omitted."""
    from greenfloor.daemon.cycle_runner import run_once

    controls = (
        dict(test_controls) if test_controls is not None else {"skip_strategy_execution": True}
    )
    if kwargs.get("coin_watchlist") is None:
        kwargs["coin_watchlist"] = new_coin_watchlist_cache()
    return run_once(*args, test_controls=controls, **kwargs)


def install_rust_cycle_test_env(monkeypatch) -> None:
    """Default env for in-process Rust daemon cycle integration tests."""
    monkeypatch.setenv("GREENFLOOR_XCH_PRICE_USD", "30")
    monkeypatch.setenv("GREENFLOOR_ENGINE_BIN", str(engine_binary_path()))

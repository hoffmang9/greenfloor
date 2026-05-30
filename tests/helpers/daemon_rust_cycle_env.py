"""Test helpers for native greenfloor-engine daemon cycle runs."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from greenfloor.core.engine_bridge import import_engine, require_engine_method


def new_coin_watchlist_cache() -> Any:
    cache_cls = require_engine_method(
        import_engine(),
        "CoinWatchlistCache",
        missing="coin watchlist cache",
    )
    return cache_cls()


def run_daemon_cycle_once(request: dict[str, Any]) -> dict[str, Any]:
    run_fn = require_engine_method(
        import_engine(),
        "run_daemon_cycle_once",
        missing="daemon cycle once",
    )
    return dict(run_fn(request))


def run_once_for_tests(
    *,
    program_path: Path,
    markets_path: Path,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
    poll_coinset_mempool: bool = True,
    use_websocket_capture: bool = False,
    testnet_markets_path: Path | None = None,
    test_controls: dict[str, object] | None = None,
) -> int:
    controls = (
        dict(test_controls) if test_controls is not None else {"skip_strategy_execution": True}
    )
    request = {
        "program_path": str(program_path),
        "markets_path": str(markets_path),
        "coinset_base_url": coinset_base_url,
        "state_dir": str(state_dir),
        "poll_coinset_mempool": poll_coinset_mempool,
        "use_websocket_capture": use_websocket_capture,
        "allowed_key_ids": sorted(allowed_keys or []),
        "dispatch_state": {"cursor": 0, "immediate_requeue_ids": []},
        "test_controls": controls,
    }
    if testnet_markets_path is not None:
        request["testnet_markets_path"] = str(testnet_markets_path)
    if db_path_override:
        request["state_db_override"] = db_path_override
    response = run_daemon_cycle_once(request)
    return int(response["exit_code"])


def install_rust_cycle_test_env(monkeypatch) -> None:
    from tests.helpers.engine_binary import engine_binary_path

    monkeypatch.setenv("GREENFLOOR_XCH_PRICE_USD", "30")
    monkeypatch.setenv("GREENFLOOR_ENGINE_BIN", str(engine_binary_path()))

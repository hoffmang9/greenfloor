"""Cycle orchestration patch points (`run_once`, `run_loop`, adapters)."""

from __future__ import annotations

from typing import Any

import greenfloor.daemon.cycle_runner as cycle_runner
from greenfloor.daemon.cycle_runner import (
    consume_reload_marker,
    resolve_cycle_websocket_capture,
    run_loop,
    run_once,
)
from greenfloor.daemon.main import _acquire_daemon_instance_lock
from greenfloor.daemon.main import main as cli_main
from greenfloor.daemon.testing.market_cycle_result import MarketCycleResult

# Tests monkeypatch adapter imports on this module object.
main = cycle_runner


def _dispatch_state_cls() -> Any:
    from greenfloor.core.engine_bridge import import_engine, require_engine_method

    return require_engine_method(
        import_engine(),
        "DaemonDispatchState",
        missing="daemon dispatch state",
    )


def __getattr__(name: str) -> Any:
    if name == "MarketDispatchState":
        return _dispatch_state_cls()
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


MarketDispatchState: Any

__all__ = [
    "MarketCycleResult",
    "MarketDispatchState",
    "_acquire_daemon_instance_lock",
    "cli_main",
    "consume_reload_marker",
    "main",
    "resolve_cycle_websocket_capture",
    "run_loop",
    "run_once",
]

"""Cycle orchestration patch points (`run_once`, `run_loop`, adapters)."""

from __future__ import annotations

import greenfloor.daemon.cycle_runner as cycle_runner
from greenfloor.daemon.cycle_market_batch import (
    MarketDispatchState,
    enqueue_immediate_requeue_market,
    select_market_batch,
)
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

__all__ = [
    "MarketCycleResult",
    "MarketDispatchState",
    "_acquire_daemon_instance_lock",
    "cli_main",
    "consume_reload_marker",
    "enqueue_immediate_requeue_market",
    "main",
    "resolve_cycle_websocket_capture",
    "run_loop",
    "run_once",
    "select_market_batch",
]

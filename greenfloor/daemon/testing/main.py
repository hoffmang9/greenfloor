"""Cycle orchestration patch points for daemon tests."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.daemon.main import main as cli_main
from greenfloor.daemon.testing.market_cycle_result import MarketCycleResult
from tests.helpers.daemon_rust_cycle_env import run_once_for_tests as run_once

_DAEMON_MISSING = "daemon cycle"


def _daemon_method(name: str):
    return require_engine_method(import_engine(), name, missing=_DAEMON_MISSING)


def consume_reload_marker(state_dir: Path) -> bool:
    return bool(_daemon_method("consume_reload_marker")(state_dir))


def _acquire_daemon_instance_lock(*, state_dir: Path, mode: str) -> Any:
    return _daemon_method("acquire_daemon_instance_lock")(state_dir, mode)


def resolve_cycle_websocket_capture(*, program, loop_websocket_active: bool) -> bool:
    if loop_websocket_active:
        return False
    mode = str(getattr(program, "tx_block_trigger_mode", "websocket"))
    return bool(_daemon_method("use_websocket_capture_for_trigger_mode")(mode))


def run_loop(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
) -> int:
    request = {
        "program_path": str(program_path),
        "markets_path": str(markets_path),
        "coinset_base_url": coinset_base_url,
        "state_dir": str(state_dir),
        "allowed_key_ids": sorted(allowed_keys or []),
    }
    if testnet_markets_path is not None:
        request["testnet_markets_path"] = str(testnet_markets_path)
    if db_path_override:
        request["state_db_override"] = db_path_override
    try:
        return int(_daemon_method("run_daemon_loop")(request))
    except KeyboardInterrupt:
        return 0


def _dispatch_state_cls() -> Any:
    return dict


MarketDispatchState = dict[str, Any]

__all__ = [
    "MarketCycleResult",
    "MarketDispatchState",
    "_acquire_daemon_instance_lock",
    "cli_main",
    "consume_reload_marker",
    "resolve_cycle_websocket_capture",
    "run_loop",
    "run_once",
]

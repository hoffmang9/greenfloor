"""Daemon cycle orchestration: single-cycle execution and the long-running loop."""

from __future__ import annotations

import logging
import os
import time
from collections.abc import Callable, Mapping
from pathlib import Path
from typing import Any

from greenfloor.config.io import load_program_config
from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.daemon.bootstrap import log_daemon_event
from greenfloor.daemon.engine_logging import initialize_daemon_logging
from greenfloor.daemon.market_logging import _daemon_logger
from greenfloor.runtime.offer_watchlist import new_coin_watchlist_cache

__all__ = [
    "consume_reload_marker",
    "new_coin_watchlist_cache",
    "resolve_cycle_state_db_path",
    "resolve_cycle_websocket_capture",
    "run_daemon_cycle_once_via_engine",
    "run_loop",
    "run_once",
]

_DAEMON_ENGINE_MISSING = "daemon cycle"


def _daemon_method(name: str):
    return require_engine_method(import_engine(), name, missing=_DAEMON_ENGINE_MISSING)


def _new_dispatch_state() -> Any:
    dispatch_cls = _daemon_method("DaemonDispatchState")
    return dispatch_cls(0, [])


def resolve_cycle_websocket_capture(*, program, loop_websocket_active: bool) -> bool:
    if loop_websocket_active:
        return False
    mode = str(getattr(program, "tx_block_trigger_mode", "websocket"))
    use_websocket = _daemon_method("use_websocket_capture_for_trigger_mode")
    return bool(use_websocket(mode))


def consume_reload_marker(state_dir: Path) -> bool:
    marker = state_dir / "reload_request.json"
    if not marker.exists():
        return False
    marker.unlink(missing_ok=True)
    return True


def resolve_cycle_state_db_path(*, program_home_dir: str, db_path_override: str | None) -> str:
    resolve = _daemon_method("resolve_state_db_path")
    return str(resolve(Path(program_home_dir).expanduser(), db_path_override))


def _build_engine_request(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
    poll_coinset_mempool: bool,
    use_websocket_capture: bool,
    dispatch_state: Any,
    coin_watchlist: Any,
    test_controls: Mapping[str, Any] | None = None,
) -> Any:
    controls_cls = _daemon_method("DaemonCycleTestControls")
    request_cls = _daemon_method("DaemonRunOnceRequest")

    forced = None
    skip_strategy = False
    if test_controls:
        skip_strategy = bool(test_controls.get("skip_strategy_execution", False))
        raw_forced = test_controls.get("force_market_error_for")
        forced = str(raw_forced) if raw_forced is not None else None

    return request_cls(
        program_path,
        markets_path,
        coinset_base_url,
        state_dir,
        testnet_markets_path=testnet_markets_path,
        state_db_override=db_path_override,
        poll_coinset_mempool=poll_coinset_mempool,
        use_websocket_capture=use_websocket_capture,
        allowed_key_ids=sorted(allowed_keys or []),
        dispatch_state=dispatch_state,
        test_controls=controls_cls(
            skip_strategy_execution=skip_strategy,
            force_market_error_for=forced,
        ),
        coin_watchlist=coin_watchlist,
    )


def run_daemon_cycle_once_via_engine(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
    poll_coinset_mempool: bool,
    use_websocket_capture: bool,
    dispatch_state: Any | None = None,
    coin_watchlist: Any,
    run_fn: Callable[[Any], Any] | None = None,
    test_controls: Mapping[str, Any] | None = None,
) -> tuple[int, Any]:
    state = dispatch_state or _new_dispatch_state()
    request = _build_engine_request(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        allowed_keys=allowed_keys,
        db_path_override=db_path_override,
        coinset_base_url=coinset_base_url,
        state_dir=state_dir,
        poll_coinset_mempool=poll_coinset_mempool,
        use_websocket_capture=use_websocket_capture,
        dispatch_state=state,
        coin_watchlist=coin_watchlist,
        test_controls=test_controls,
    )

    runner = run_fn or _daemon_method("run_daemon_cycle_once")
    response = runner(request)
    exit_code = int(getattr(response, "exit_code", 1))
    updated = getattr(response, "dispatch_state", None)
    if updated is None:
        raise TypeError("engine run_daemon_cycle_once returned response without dispatch_state")
    return exit_code, updated


def run_once(
    program_path: Path,
    markets_path: Path,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
    poll_coinset_mempool: bool = True,
    use_websocket_capture: bool = False,
    testnet_markets_path: Path | None = None,
    dispatch_state: Any | None = None,
    *,
    coin_watchlist: Any,
    test_controls: Mapping[str, Any] | None = None,
) -> int:
    state = dispatch_state or _new_dispatch_state()
    exit_code, updated_state = run_daemon_cycle_once_via_engine(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        allowed_keys=allowed_keys,
        db_path_override=db_path_override,
        coinset_base_url=coinset_base_url,
        state_dir=state_dir,
        poll_coinset_mempool=poll_coinset_mempool,
        use_websocket_capture=use_websocket_capture,
        dispatch_state=state,
        coin_watchlist=coin_watchlist,
        test_controls=test_controls,
    )
    if dispatch_state is not None:
        dispatch_state.cursor = int(updated_state.cursor)
        dispatch_state.immediate_requeue_ids = list(updated_state.immediate_requeue_ids)
    return exit_code


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
    current_program = load_program_config(program_path)
    dispatch_state = _new_dispatch_state()
    coin_watchlist = new_coin_watchlist_cache()
    initialize_daemon_logging(program=current_program, program_path=program_path)
    _daemon_logger.info(
        "daemon_starting mode=loop program_config=%s markets_config=%s",
        os.fspath(program_path),
        os.fspath(markets_path),
    )
    db_path = resolve_cycle_state_db_path(
        program_home_dir=current_program.home_dir,
        db_path_override=db_path_override,
    )
    start_ws_loop = _daemon_method("start_coinset_websocket_loop")
    ws_client = start_ws_loop(db_path, program_path, coinset_base_url, coin_watchlist)

    try:
        while True:
            initialize_daemon_logging(program=current_program, program_path=program_path)
            exit_code = run_once(
                program_path=program_path,
                markets_path=markets_path,
                allowed_keys=allowed_keys,
                db_path_override=db_path_override,
                coinset_base_url=coinset_base_url,
                state_dir=state_dir,
                poll_coinset_mempool=False,
                use_websocket_capture=False,
                testnet_markets_path=testnet_markets_path,
                dispatch_state=dispatch_state,
                coin_watchlist=coin_watchlist,
            )
            if exit_code != 0:
                _daemon_logger.warning("daemon_cycle_exit_code=%s", exit_code)
            if consume_reload_marker(state_dir):
                log_daemon_event(level=logging.INFO, payload={"event": "config_reloaded"})
            time.sleep(max(1, current_program.runtime_loop_interval_seconds))
            current_program = load_program_config(program_path)
    except KeyboardInterrupt:
        return 0
    finally:
        ws_client.stop()
        _daemon_logger.info("daemon_stopped mode=loop")
    return 0

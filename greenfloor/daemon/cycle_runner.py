"""Daemon cycle orchestration: single-cycle execution and the long-running loop."""

from __future__ import annotations

from collections.abc import Callable, Mapping
from pathlib import Path
from typing import Any

from greenfloor.config.io import load_program_config
from greenfloor.core.daemon_engine_types import (
    CoinWatchlistCache,
    DaemonDispatchState,
    DaemonLoopRequest,
    DaemonRunOnceRequest,
)
from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.daemon.engine_logging import initialize_daemon_logging

__all__ = [
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


def new_coin_watchlist_cache() -> CoinWatchlistCache:
    cache_cls = _daemon_method("CoinWatchlistCache")
    return cache_cls()


def _new_dispatch_state() -> DaemonDispatchState:
    dispatch_cls = _daemon_method("DaemonDispatchState")
    return dispatch_cls(0, [])


def resolve_cycle_websocket_capture(*, program, loop_websocket_active: bool) -> bool:
    if loop_websocket_active:
        return False
    mode = str(getattr(program, "tx_block_trigger_mode", "websocket"))
    use_websocket = _daemon_method("use_websocket_capture_for_trigger_mode")
    return bool(use_websocket(mode))


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
    dispatch_state: DaemonDispatchState,
    coin_watchlist: CoinWatchlistCache,
    test_controls: Mapping[str, object] | None = None,
) -> DaemonRunOnceRequest:
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
        coin_watchlist,
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
    dispatch_state: DaemonDispatchState,
    coin_watchlist: CoinWatchlistCache,
    run_fn: Callable[[DaemonRunOnceRequest], Any] | None = None,
    test_controls: Mapping[str, object] | None = None,
) -> tuple[int, DaemonDispatchState]:
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
        dispatch_state=dispatch_state,
        coin_watchlist=coin_watchlist,
        test_controls=test_controls,
    )

    runner = run_fn or _daemon_method("run_daemon_cycle_once")
    response = runner(request)
    updated = response.dispatch_state
    return int(response.exit_code), updated


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
    dispatch_state: DaemonDispatchState | None = None,
    *,
    coin_watchlist: CoinWatchlistCache,
    test_controls: Mapping[str, object] | None = None,
) -> int:
    state = dispatch_state or _new_dispatch_state()
    exit_code, _updated = run_daemon_cycle_once_via_engine(
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
    program = load_program_config(program_path)
    initialize_daemon_logging(program=program, program_path=program_path)

    loop_request_cls = _daemon_method("DaemonLoopRequest")
    run_loop_fn = _daemon_method("run_daemon_loop")
    request: DaemonLoopRequest = loop_request_cls(
        program_path,
        markets_path,
        coinset_base_url,
        state_dir,
        testnet_markets_path=testnet_markets_path,
        state_db_override=db_path_override,
        allowed_key_ids=sorted(allowed_keys or []),
    )
    try:
        return int(run_loop_fn(request))
    except KeyboardInterrupt:
        return 0

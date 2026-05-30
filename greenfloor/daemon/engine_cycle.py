"""In-process Rust daemon cycle orchestration via PyO3."""

from __future__ import annotations

from collections.abc import Callable, Mapping
from pathlib import Path
from typing import Any

from greenfloor.core.engine_bridge import import_engine, require_engine_method


def _engine_module():
    return import_engine()


def _require(name: str, *, missing: str):
    return require_engine_method(_engine_module(), name, missing=missing)


def _default_dispatch_state() -> Any:
    dispatch_cls = _require("DaemonDispatchState", missing="daemon dispatch state")
    return dispatch_cls(0, [])


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
    controls_cls = _require("DaemonCycleTestControls", missing="daemon cycle test controls")
    request_cls = _require("DaemonRunOnceRequest", missing="daemon cycle request")

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
    state = dispatch_state or _default_dispatch_state()
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

    runner = run_fn or _require("run_daemon_cycle_once", missing="daemon cycle")
    response = runner(request)
    exit_code = int(getattr(response, "exit_code", 1))
    updated = getattr(response, "dispatch_state", None)
    if updated is None:
        raise TypeError("engine run_daemon_cycle_once returned response without dispatch_state")
    return exit_code, updated

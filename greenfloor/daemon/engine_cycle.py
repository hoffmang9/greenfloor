"""In-process Rust daemon cycle orchestration via PyO3."""

from __future__ import annotations

from collections import deque
from collections.abc import Callable, Mapping
from pathlib import Path
from typing import Any

from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.daemon.cycle_market_batch import MarketDispatchState


def _engine_module():
    return import_engine()


def _run_daemon_cycle_once_engine():
    return require_engine_method(
        _engine_module(),
        "run_daemon_cycle_once",
        missing="daemon cycle",
    )


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
    dispatch_state: MarketDispatchState,
    test_controls: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    controls: dict[str, Any] = {}
    if test_controls:
        controls["skip_strategy_execution"] = bool(
            test_controls.get("skip_strategy_execution", False)
        )
        forced = test_controls.get("force_market_error_for")
        controls["force_market_error_for"] = str(forced) if forced is not None else None

    return {
        "program_path": str(program_path),
        "markets_path": str(markets_path),
        "testnet_markets_path": str(testnet_markets_path) if testnet_markets_path else None,
        "state_db_override": db_path_override,
        "coinset_base_url": coinset_base_url,
        "state_dir": str(state_dir),
        "poll_coinset_mempool": poll_coinset_mempool,
        "use_websocket_capture": use_websocket_capture,
        "allowed_key_ids": sorted(allowed_keys or []),
        "dispatch_state": {
            "cursor": int(dispatch_state.cursor),
            "immediate_requeue_ids": list(dispatch_state.immediate_requeue_ids),
        },
        "test_controls": controls,
    }


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
    market_dispatch_state: MarketDispatchState | None,
    run_fn: Callable[[Any], Any] | None = None,
    test_controls: Mapping[str, Any] | None = None,
) -> tuple[int, MarketDispatchState]:
    dispatch_state = market_dispatch_state or MarketDispatchState()
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
        test_controls=test_controls,
    )

    runner = run_fn or _run_daemon_cycle_once_engine()
    response = runner(request)
    if not isinstance(response, dict):
        raise TypeError("engine run_daemon_cycle_once must return a dict")
    exit_code = int(response.get("exit_code", 1))
    state_payload = response.get("dispatch_state")
    if not isinstance(state_payload, dict):
        raise TypeError("engine run_daemon_cycle_once returned response without dispatch_state")
    updated = MarketDispatchState(
        cursor=int(state_payload.get("cursor", dispatch_state.cursor)),
        immediate_requeue_ids=deque(
            str(market_id) for market_id in state_payload.get("immediate_requeue_ids", [])
        ),
    )
    return exit_code, updated

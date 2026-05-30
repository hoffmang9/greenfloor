"""In-process Rust daemon cycle orchestration."""

from __future__ import annotations

from collections import deque
from pathlib import Path
from typing import Any, cast

from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.daemon.cycle_market_batch import MarketDispatchState
from greenfloor.runtime.daemon_config_paths import DaemonConfigPaths, set_daemon_config_paths


def _engine_run_daemon_cycle_once() -> Any:
    return require_engine_method(
        import_engine(),
        "run_daemon_cycle_once",
        missing="run_daemon_cycle_once",
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
    market_dispatch_state: MarketDispatchState | None,
) -> tuple[int, MarketDispatchState]:
    set_daemon_config_paths(
        DaemonConfigPaths(
            program_path=program_path,
            markets_path=markets_path,
            testnet_markets_path=testnet_markets_path,
        )
    )
    dispatch_state = market_dispatch_state or MarketDispatchState()
    request: dict[str, Any] = {
        "program_path": str(program_path),
        "markets_path": str(markets_path),
        "state_db_override": db_path_override or "",
        "coinset_base_url": coinset_base_url,
        "state_dir": str(state_dir),
        "poll_coinset_mempool": bool(poll_coinset_mempool),
        "use_websocket_capture": bool(use_websocket_capture),
        "allowed_key_ids": sorted(allowed_keys or []),
        "dispatch_state": {
            "cursor": int(dispatch_state.cursor),
            "immediate_requeue_ids": list(dispatch_state.immediate_requeue_ids),
        },
    }
    if testnet_markets_path is not None:
        request["testnet_markets_path"] = str(testnet_markets_path)

    response = _engine_run_daemon_cycle_once()(request)
    if not isinstance(response, dict):
        raise TypeError("engine run_daemon_cycle_once returned non-dict response")
    exit_code = int(response.get("exit_code", 1))
    state_payload = response.get("dispatch_state", {})
    if not isinstance(state_payload, dict):
        raise TypeError("engine run_daemon_cycle_once dispatch_state is not a dict")
    updated = MarketDispatchState(
        cursor=int(state_payload.get("cursor", dispatch_state.cursor)),
        immediate_requeue_ids=deque(
            str(market_id)
            for market_id in cast(list[Any], state_payload.get("immediate_requeue_ids", []))
        ),
    )
    return exit_code, updated

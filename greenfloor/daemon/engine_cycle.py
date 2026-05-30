"""In-process Rust daemon cycle orchestration via PyO3."""

from __future__ import annotations

from collections import deque
from collections.abc import Callable
from pathlib import Path
from typing import Any, cast

from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.daemon.cycle_market_batch import MarketDispatchState


def _run_daemon_cycle_once_engine() -> Callable[[dict[str, Any]], dict[str, Any]]:
    return cast(
        Callable[[dict[str, Any]], dict[str, Any]],
        require_engine_method(
            import_engine(),
            "run_daemon_cycle_once",
            missing="daemon cycle",
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
    market_dispatch_state: MarketDispatchState | None,
    run_fn: Callable[[dict[str, Any]], dict[str, Any]] | None = None,
) -> tuple[int, MarketDispatchState]:
    dispatch_state = market_dispatch_state or MarketDispatchState()
    request: dict[str, Any] = {
        "program_path": str(program_path),
        "markets_path": str(markets_path),
        "coinset_base_url": coinset_base_url,
        "state_dir": str(state_dir),
        "poll_coinset_mempool": poll_coinset_mempool,
        "use_websocket_capture": use_websocket_capture,
        "allowed_key_ids": sorted(allowed_keys or []),
        "dispatch_state": {
            "cursor": int(dispatch_state.cursor),
            "immediate_requeue_ids": list(dispatch_state.immediate_requeue_ids),
        },
    }
    if testnet_markets_path is not None:
        request["testnet_markets_path"] = str(testnet_markets_path)
    if db_path_override:
        request["state_db_override"] = db_path_override

    runner = run_fn or _run_daemon_cycle_once_engine()
    payload = runner(request)
    if not isinstance(payload, dict):
        raise TypeError("engine run_daemon_cycle_once returned non-object response")
    exit_code = int(payload.get("exit_code", 1))
    state_payload = payload.get("dispatch_state", {})
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

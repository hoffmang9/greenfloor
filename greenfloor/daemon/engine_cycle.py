"""In-process Rust daemon cycle orchestration via native greenfloor-engine binary."""

from __future__ import annotations

import json
import subprocess
from collections import deque
from collections.abc import Callable
from pathlib import Path
from typing import Any, cast

from greenfloor.cli.engine_binary import (
    daemon_run_once_argv,
    resolve_greenfloor_engine_binary,
)
from greenfloor.daemon.cycle_market_batch import MarketDispatchState


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
    run_fn: Callable[..., subprocess.CompletedProcess[str]] | None = None,
) -> tuple[int, MarketDispatchState]:
    dispatch_state = market_dispatch_state or MarketDispatchState()
    binary = resolve_greenfloor_engine_binary()
    argv = daemon_run_once_argv(
        binary=binary,
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        key_ids=",".join(sorted(allowed_keys or [])),
        state_db=db_path_override or "",
        coinset_base_url=coinset_base_url,
        state_dir=state_dir,
        poll_coinset_mempool=poll_coinset_mempool,
        use_websocket_capture=use_websocket_capture,
        dispatch_cursor=int(dispatch_state.cursor),
        dispatch_requeue_ids=",".join(dispatch_state.immediate_requeue_ids),
        json_output=True,
    )
    runner = run_fn or subprocess.run
    completed = runner(argv, check=False, capture_output=True, text=True)
    returncode = int(getattr(completed, "returncode", 1))
    stdout = str(getattr(completed, "stdout", "") or "").strip()
    if not stdout:
        raise RuntimeError(
            "greenfloor-engine daemon run-once --json returned empty stdout; "
            f"stderr={getattr(completed, 'stderr', '')!r}"
        )
    payload = json.loads(stdout)
    if not isinstance(payload, dict):
        raise TypeError("engine daemon run-once --json returned non-object response")
    exit_code = int(payload.get("exit_code", returncode))
    state_payload = payload.get("dispatch_state", {})
    if not isinstance(state_payload, dict):
        raise TypeError("engine daemon run-once dispatch_state is not a dict")
    updated = MarketDispatchState(
        cursor=int(state_payload.get("cursor", dispatch_state.cursor)),
        immediate_requeue_ids=deque(
            str(market_id)
            for market_id in cast(list[Any], state_payload.get("immediate_requeue_ids", []))
        ),
    )
    return exit_code, updated

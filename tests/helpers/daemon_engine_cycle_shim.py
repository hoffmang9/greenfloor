"""Test shim: run daemon cycle via Python bridge without Rust plan/build."""

from __future__ import annotations

from collections import deque
from pathlib import Path
from typing import Any

from greenfloor.config.io import load_markets_config_with_optional_overlay, load_program_config
from greenfloor.daemon.cycle_market_batch import MarketDispatchState
from greenfloor.daemon.rust_cycle_bridge import execute_market_dispatch, run_cycle_preamble
from greenfloor.runtime.daemon_config_paths import DaemonConfigPaths, set_daemon_config_paths
from greenfloor.storage.sqlite import SqliteStore


def run_daemon_cycle_once_via_bridge(request: dict[str, Any]) -> dict[str, Any]:
    program_path = Path(str(request["program_path"]))
    markets_path = Path(str(request["markets_path"]))
    testnet_path_raw = str(request.get("testnet_markets_path", "") or "").strip()
    testnet_markets_path = Path(testnet_path_raw) if testnet_path_raw else None
    state_dir = Path(str(request["state_dir"]))
    db_override = str(request.get("state_db_override", "") or "").strip() or None

    set_daemon_config_paths(
        DaemonConfigPaths(
            program_path=program_path,
            markets_path=markets_path,
            testnet_markets_path=testnet_markets_path,
        )
    )
    program = load_program_config(program_path)
    from greenfloor.config.io import resolve_state_db_path

    db_path = resolve_state_db_path(
        program_home_dir=program.home_dir,
        explicit_db_path=db_override,
    )
    store_for_previous = SqliteStore(db_path)
    try:
        previous_xch_price_usd = store_for_previous.get_latest_xch_price_snapshot()
    finally:
        store_for_previous.close()
    preamble = run_cycle_preamble(
        program_path=str(program_path),
        db_path=str(db_path),
        coinset_base_url=str(request.get("coinset_base_url", "")),
        poll_coinset_mempool=bool(request.get("poll_coinset_mempool", True)),
        use_websocket_capture=bool(request.get("use_websocket_capture", False)),
    )
    markets = load_markets_config_with_optional_overlay(
        path=markets_path,
        overlay_path=testnet_markets_path,
    )
    selected_market_ids = [
        str(market.market_id)
        for market in markets.markets
        if market.enabled and str(market.market_id).strip()
    ]
    metrics = execute_market_dispatch(
        program_path=str(program_path),
        markets_path=str(markets_path),
        testnet_markets_path=str(testnet_markets_path) if testnet_markets_path else None,
        selected_market_ids=selected_market_ids,
        allowed_key_ids=list(request.get("allowed_key_ids", [])),
        db_path=str(db_path),
        state_dir=str(state_dir),
        xch_price_usd=preamble.get("xch_price_usd"),
        previous_xch_price_usd=previous_xch_price_usd,
        parallel_markets_enabled=bool(program.runtime_parallel_markets),
    )
    summary = {
        "markets_attempted": len(selected_market_ids),
        "markets_processed": metrics["markets_processed"],
        "error_count": int(preamble.get("cycle_error_count", 0))
        + int(metrics["cycle_error_count"]),
    }
    store = __import__("greenfloor.storage.sqlite", fromlist=["SqliteStore"]).SqliteStore(db_path)
    try:
        store.add_audit_event("daemon_cycle_summary", summary)
    finally:
        store.close()
    dispatch_state = request.get("dispatch_state", {})
    return {
        "exit_code": 0,
        "dispatch_state": {
            "cursor": int(dispatch_state.get("cursor", 0)),
            "immediate_requeue_ids": list(dispatch_state.get("immediate_requeue_ids", [])),
        },
        "cycle_summary": {
            "markets_processed": metrics["markets_processed"],
            "error_count": int(preamble.get("cycle_error_count", 0))
            + int(metrics["cycle_error_count"]),
        },
    }


def bridge_dispatch_state_from_response(response: dict[str, Any]) -> MarketDispatchState:
    payload = response.get("dispatch_state", {})
    if not isinstance(payload, dict):
        raise TypeError("dispatch_state must be a dict")
    return MarketDispatchState(
        cursor=int(payload.get("cursor", 0)),
        immediate_requeue_ids=deque(
            str(market_id) for market_id in payload.get("immediate_requeue_ids", [])
        ),
    )

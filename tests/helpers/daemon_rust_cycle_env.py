"""Test helpers for native greenfloor-engine daemon cycle runs."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

from greenfloor.engine_binary import resolve_greenfloor_engine_binary


def run_once_for_tests(
    *,
    program_path: Path,
    markets_path: Path,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
    poll_coinset_mempool: bool = True,
    use_websocket_capture: bool = False,
    testnet_markets_path: Path | None = None,
    test_controls: dict[str, object] | None = None,
) -> int:
    controls = (
        dict(test_controls) if test_controls is not None else {"skip_strategy_execution": True}
    )
    request: dict[str, object] = {
        "program_path": str(program_path),
        "markets_path": str(markets_path),
        "coinset_base_url": coinset_base_url,
        "state_dir": str(state_dir),
        "poll_coinset_mempool": poll_coinset_mempool,
        "use_websocket_capture": use_websocket_capture,
        "allowed_key_ids": sorted(allowed_keys or []),
        "dispatch_state": {"cursor": 0, "immediate_requeue_ids": []},
        "test_controls": controls,
    }
    if testnet_markets_path is not None:
        request["testnet_markets_path"] = str(testnet_markets_path)
    if db_path_override:
        request["state_db_override"] = db_path_override

    state_dir.mkdir(parents=True, exist_ok=True)
    request_path = state_dir / ".once_request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")

    cmd = [
        str(resolve_greenfloor_engine_binary()),
        "daemon-once",
        "--request-json",
        str(request_path),
        "--json",
    ]
    result = subprocess.run(cmd, check=False, capture_output=True, text=True)
    if result.returncode not in {0, 1, 3} and result.stderr.strip():
        raise RuntimeError(
            f"greenfloor-engine daemon-once failed (exit {result.returncode}): "
            f"{result.stderr.strip()}"
        )
    return int(result.returncode)


def install_rust_cycle_test_env(monkeypatch) -> None:
    monkeypatch.setenv("GREENFLOOR_XCH_PRICE_USD", "30")
    monkeypatch.setenv(
        "GREENFLOOR_ENGINE_BIN",
        str(resolve_greenfloor_engine_binary()),
    )

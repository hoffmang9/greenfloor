"""Tests for Rust daemon cycle orchestration delegation."""

from __future__ import annotations

import json
import subprocess
from collections import deque
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

import pytest

from greenfloor.daemon.cycle_market_batch import MarketDispatchState
from greenfloor.daemon.cycle_runner import run_once
from greenfloor.daemon.engine_cycle import run_daemon_cycle_once_via_engine


def _fake_completed(payload: Any, *, returncode: int = 0) -> subprocess.CompletedProcess[str]:
    return subprocess.CompletedProcess(
        args=["greenfloor-engine"],
        returncode=returncode,
        stdout=json.dumps(payload),
        stderr="",
    )


def test_run_daemon_cycle_once_via_engine_delegates_to_engine_binary(
    monkeypatch, tmp_path: Path
) -> None:
    captured: dict[str, list[str]] = {}
    dispatch_state = MarketDispatchState(cursor=2, immediate_requeue_ids=deque(["m-old"]))

    def _fake_run(argv: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        captured["argv"] = argv
        return _fake_completed(
            {
                "exit_code": 0,
                "dispatch_state": {
                    "cursor": 3,
                    "immediate_requeue_ids": ["m-new"],
                },
            }
        )

    monkeypatch.setattr(
        "greenfloor.daemon.engine_cycle.resolve_greenfloor_engine_binary",
        lambda: tmp_path / "greenfloor-engine",
    )
    monkeypatch.setattr("greenfloor.daemon.engine_cycle.subprocess.run", _fake_run)

    exit_code, updated = run_daemon_cycle_once_via_engine(
        program_path=tmp_path / "program.yaml",
        markets_path=tmp_path / "markets.yaml",
        testnet_markets_path=None,
        allowed_keys={"key-a"},
        db_path_override=None,
        coinset_base_url="https://api.coinset.org",
        state_dir=tmp_path / "state",
        poll_coinset_mempool=False,
        use_websocket_capture=True,
        market_dispatch_state=dispatch_state,
    )
    assert exit_code == 0
    assert updated.cursor == 3
    assert updated.immediate_requeue_ids == deque(["m-new"])
    argv = captured["argv"]
    assert "--json" in argv
    assert "--dispatch-cursor" in argv
    assert "--use-websocket-capture" in argv


def test_run_once_is_thin_wrapper_over_engine_cycle(monkeypatch, tmp_path: Path) -> None:
    mock = MagicMock(return_value=(0, MarketDispatchState()))
    monkeypatch.setattr("greenfloor.daemon.cycle_runner.run_daemon_cycle_once_via_engine", mock)
    code = run_once(
        tmp_path / "program.yaml",
        tmp_path / "markets.yaml",
        {"key-a"},
        None,
        "https://api.coinset.org",
        tmp_path / "state",
        poll_coinset_mempool=True,
        use_websocket_capture=False,
    )
    assert code == 0
    mock.assert_called_once()


def test_run_daemon_cycle_once_requires_json_object(monkeypatch, tmp_path: Path) -> None:
    monkeypatch.setattr(
        "greenfloor.daemon.engine_cycle.resolve_greenfloor_engine_binary",
        lambda: tmp_path / "greenfloor-engine",
    )
    monkeypatch.setattr(
        "greenfloor.daemon.engine_cycle.subprocess.run",
        lambda *_args, **_kwargs: _fake_completed('"not-a-dict"'),
    )
    with pytest.raises(TypeError, match="non-object"):
        run_daemon_cycle_once_via_engine(
            program_path=tmp_path / "program.yaml",
            markets_path=tmp_path / "markets.yaml",
            testnet_markets_path=None,
            allowed_keys=None,
            db_path_override=None,
            coinset_base_url="https://api.coinset.org",
            state_dir=tmp_path / "state",
            poll_coinset_mempool=False,
            use_websocket_capture=False,
            market_dispatch_state=None,
        )

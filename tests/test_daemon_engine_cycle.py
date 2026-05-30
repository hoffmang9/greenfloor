"""Tests for Rust daemon cycle orchestration delegation."""

from __future__ import annotations

from collections import deque
from collections.abc import Mapping
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

import pytest

from greenfloor.daemon.cycle_market_batch import MarketDispatchState
from greenfloor.daemon.cycle_runner import run_once
from greenfloor.daemon.engine_cycle import run_daemon_cycle_once_via_engine


def test_run_daemon_cycle_once_via_engine_delegates_to_pyo3(monkeypatch, tmp_path: Path) -> None:
    captured: dict[str, Any] = {}
    dispatch_state = MarketDispatchState(cursor=2, immediate_requeue_ids=deque(["m-old"]))

    def _fake_run(request: Mapping[str, Any]) -> dict[str, Any]:
        captured["request"] = request
        return {
            "exit_code": 0,
            "dispatch_state": {
                "cursor": 3,
                "immediate_requeue_ids": ["m-new"],
            },
        }

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
        run_fn=_fake_run,
    )
    assert exit_code == 0
    assert updated.cursor == 3
    assert updated.immediate_requeue_ids == deque(["m-new"])
    request = captured["request"]
    assert request["poll_coinset_mempool"] is False
    assert request["use_websocket_capture"] is True
    assert request["dispatch_state"]["cursor"] == 2
    assert request["allowed_key_ids"] == ["key-a"]


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


def test_run_daemon_cycle_once_requires_json_object(tmp_path: Path) -> None:
    def _bad_run(_request: dict[str, Any]) -> str:
        return "not-a-dict"

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
            run_fn=_bad_run,  # type: ignore[arg-type]
        )

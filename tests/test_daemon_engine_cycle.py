"""Tests for Rust daemon cycle orchestration delegation."""

from __future__ import annotations

from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

from tests.helpers.daemon_rust_cycle_env import run_once_for_tests


def test_run_daemon_cycle_once_delegates_via_json_request(monkeypatch, tmp_path: Path) -> None:
    captured: dict[str, Any] = {}

    def _fake_run(request: dict[str, Any]) -> dict[str, Any]:
        captured["request"] = request
        return {
            "exit_code": 0,
            "dispatch_state": {"cursor": 3, "immediate_requeue_ids": ["m-new"]},
            "cycle_summary": {"markets_processed": 1},
        }

    monkeypatch.setattr(
        "tests.helpers.daemon_rust_cycle_env.run_daemon_cycle_once",
        _fake_run,
    )

    code = run_once_for_tests(
        program_path=tmp_path / "program.yaml",
        markets_path=tmp_path / "markets.yaml",
        allowed_keys={"key-a"},
        db_path_override=None,
        coinset_base_url="https://api.coinset.org",
        state_dir=tmp_path / "state",
        poll_coinset_mempool=False,
        use_websocket_capture=True,
    )
    assert code == 0
    request = captured["request"]
    assert request["poll_coinset_mempool"] is False
    assert request["use_websocket_capture"] is True
    assert request["allowed_key_ids"] == ["key-a"]


def test_run_once_for_tests_is_thin_wrapper(monkeypatch, tmp_path: Path) -> None:
    mock = MagicMock(return_value={"exit_code": 0})
    monkeypatch.setattr(
        "tests.helpers.daemon_rust_cycle_env.run_daemon_cycle_once",
        mock,
    )
    code = run_once_for_tests(
        program_path=tmp_path / "program.yaml",
        markets_path=tmp_path / "markets.yaml",
        allowed_keys={"key-a"},
        db_path_override=None,
        coinset_base_url="https://api.coinset.org",
        state_dir=tmp_path / "state",
    )
    assert code == 0
    mock.assert_called_once()

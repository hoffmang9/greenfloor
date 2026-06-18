from __future__ import annotations

from pathlib import Path

import pytest

from tests.helpers.daemon_rust_cycle_env import run_once_for_tests as run_once
from tests.helpers.daemon_websocket_fixtures import write_markets_two, write_program


@pytest.fixture
def rust_cycle_test_env(monkeypatch: pytest.MonkeyPatch) -> None:
    from tests.helpers.daemon_rust_cycle_env import install_rust_cycle_test_env

    install_rust_cycle_test_env(monkeypatch)


def test_daemon_once_processes_multiple_markets(rust_cycle_test_env: None, tmp_path: Path) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home)
    write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    result = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert result.exit_code == 0
    assert result.response is not None
    cycle_summary = result.response.get("cycle_summary")
    assert isinstance(cycle_summary, dict)
    assert cycle_summary.get("markets_processed") == 2
    assert cycle_summary.get("markets_attempted") == 2


def test_daemon_once_isolates_forced_market_error(
    rust_cycle_test_env: None, tmp_path: Path
) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home)
    write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    result = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
        test_controls={
            "skip_strategy_execution": True,
            "force_market_error_for": "m1",
        },
    )
    assert result.exit_code == 0
    assert result.response is not None
    cycle_summary = result.response.get("cycle_summary")
    assert isinstance(cycle_summary, dict)
    assert cycle_summary.get("markets_attempted") == 2
    assert cycle_summary.get("markets_processed") == 1
    assert int(cycle_summary.get("error_count", 0)) >= 1

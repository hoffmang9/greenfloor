from __future__ import annotations

import time
from collections.abc import Generator
from pathlib import Path

import pytest

from greenfloor.daemon.testing import run_once
from tests.helpers.daemon_websocket_fixtures import write_markets, write_program
from tests.helpers.dexie_http_mock import DexieHttpMock
from tests.helpers.engine_binary import engine_binary_path


@pytest.fixture
def dexie_mock() -> Generator[DexieHttpMock, None, None]:
    mock = DexieHttpMock()
    mock.start()
    try:
        yield mock
    finally:
        mock.stop()


@pytest.fixture(autouse=True)
def rust_cycle_test_env(monkeypatch) -> None:
    monkeypatch.setenv("GREENFLOOR_XCH_PRICE_USD", "30")
    monkeypatch.setenv("GREENFLOOR_ENGINE_BIN", str(engine_binary_path()))

    import greenfloor.daemon.cycle_runner as cycle_runner
    import greenfloor.daemon.testing.main as testing_main

    original_run_once = cycle_runner.run_once

    def _run_once_with_test_defaults(*args, test_controls=None, **kwargs):
        controls = (
            dict(test_controls) if test_controls is not None else {"skip_strategy_execution": True}
        )
        return original_run_once(*args, test_controls=controls, **kwargs)

    monkeypatch.setattr(cycle_runner, "run_once", _run_once_with_test_defaults)
    monkeypatch.setattr(testing_main, "run_once", _run_once_with_test_defaults)


def test_daemon_cycle_completes_under_one_second(tmp_path: Path, dexie_mock: DexieHttpMock) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, dexie_api_base=dexie_mock.base_url)
    write_markets(markets)

    started = time.monotonic()
    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    elapsed = time.monotonic() - started
    assert code == 0
    assert elapsed < 1.0

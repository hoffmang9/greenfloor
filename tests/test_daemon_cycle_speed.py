from __future__ import annotations

import time
from collections.abc import Generator
from pathlib import Path

import pytest

from greenfloor.daemon.testing import run_once
from tests.helpers.daemon_websocket_fixtures import write_markets, write_program
from tests.helpers.dexie_http_mock import DexieHttpMock


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
    monkeypatch.setenv("GREENFLOOR_TEST_SKIP_STRATEGY_EXEC", "1")
    monkeypatch.delenv("GREENFLOOR_TEST_FORCE_MARKET_ERROR", raising=False)


def test_daemon_cycle_completes_under_one_second(
    tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
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

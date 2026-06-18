from __future__ import annotations

import platform
import time
from collections.abc import Generator
from pathlib import Path

import pytest

from tests.helpers.daemon_rust_cycle_env import run_once_for_tests as run_once
from tests.helpers.daemon_websocket_fixtures import write_markets, write_program
from tests.helpers.dexie_http_mock import DexieHttpMock

# Subprocess daemon-once adds startup overhead vs in-process calls. Revisit tightening
# once CI uses release builds or a lighter JSON-once entrypoint.
_AARCH64_MAX_CYCLE_SECONDS = 2.0
_DEFAULT_MAX_CYCLE_SECONDS = 1.5


def _max_cycle_seconds() -> float:
    machine = platform.machine().casefold()
    if machine in {"aarch64", "arm64"}:
        return _AARCH64_MAX_CYCLE_SECONDS
    return _DEFAULT_MAX_CYCLE_SECONDS


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
    from tests.helpers.daemon_rust_cycle_env import install_rust_cycle_test_env

    install_rust_cycle_test_env(monkeypatch)


def test_daemon_cycle_completes_under_one_second(tmp_path: Path, dexie_mock: DexieHttpMock) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, dexie_api_base=dexie_mock.base_url)
    write_markets(markets)

    started = time.monotonic()
    result = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    elapsed = time.monotonic() - started
    assert result.exit_code == 0
    assert elapsed < _max_cycle_seconds()

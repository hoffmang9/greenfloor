from __future__ import annotations

from collections.abc import Generator
from pathlib import Path

import pytest

from tests.helpers.daemon_rust_cycle_env import run_once_for_tests as run_once
from tests.helpers.daemon_websocket_fixtures import (
    write_markets,
    write_markets_two,
    write_program,
)
from tests.helpers.dexie_http_mock import DexieHttpMock
from tests.helpers.sqlite_store import SqliteStore


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


def test_run_once_multi_market_sequential_execution(
    tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, dexie_api_base=dexie_mock.base_url)
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

    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(event_types=["daemon_cycle_summary"], limit=1)
    finally:
        store.close()
    assert len(events) == 1
    payload = events[0]["payload"]
    assert payload["markets_attempted"] == 2
    assert payload["markets_processed"] == 2


def test_run_once_multi_market_failure_isolated(tmp_path: Path, dexie_mock: DexieHttpMock) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, dexie_api_base=dexie_mock.base_url)
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

    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(event_types=["daemon_cycle_summary"], limit=1)
    finally:
        store.close()
    assert len(events) == 1
    payload = events[0]["payload"]
    assert payload["markets_attempted"] == 2
    assert payload["markets_processed"] == 1
    assert payload["error_count"] >= 1


def test_run_once_sequential_slot_rotation_picks_up_new_market_next_cycle(
    tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, dexie_api_base=dexie_mock.base_url)
    write_markets(markets)
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

    write_markets_two(markets)
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

    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(event_types=["daemon_cycle_summary"], limit=2)
    finally:
        store.close()
    attempted = sorted(int(e["payload"]["markets_attempted"]) for e in events)
    processed = sorted(int(e["payload"]["markets_processed"]) for e in events)
    assert attempted == [1, 2]
    assert processed == [1, 2]


def test_run_once_all_markets_fail_exits_non_zero(
    tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, dexie_api_base=dexie_mock.base_url)
    write_markets(markets)
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
    assert result.exit_code == 1

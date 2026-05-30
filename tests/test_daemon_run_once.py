from __future__ import annotations

from collections.abc import Generator
from pathlib import Path

import pytest

from greenfloor.daemon.testing import run_once
from greenfloor.storage.sqlite import SqliteStore
from tests.helpers.daemon_websocket_fixtures import (
    write_markets,
    write_markets_two,
    write_program,
)
from tests.helpers.dexie_http_mock import DexieHttpMock
from tests.helpers.engine_binary import engine_binary_path
from tests.logging_helpers import reset_concurrent_log_handlers


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
    monkeypatch.setenv("GREENFLOOR_ENGINE_BIN", str(engine_binary_path()))


def test_run_once_parallel_markets_overlap_execution(
    tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, parallel_markets=True, dexie_api_base=dexie_mock.base_url)
    write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 0

    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(event_types=["daemon_cycle_summary"], limit=1)
    finally:
        store.close()
    assert len(events) == 1
    payload = events[0]["payload"]
    assert payload["markets_attempted"] == 2
    assert payload["markets_processed"] == 2


def test_run_once_parallel_market_failure_isolated(
    monkeypatch, tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
    monkeypatch.setenv("GREENFLOOR_TEST_FORCE_MARKET_ERROR", "m1")
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, parallel_markets=True, dexie_api_base=dexie_mock.base_url)
    write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 0

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


def test_run_once_sequential_market_failure_isolated(
    monkeypatch, tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
    monkeypatch.setenv("GREENFLOOR_TEST_FORCE_MARKET_ERROR", "m1")
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, parallel_markets=False, dexie_api_base=dexie_mock.base_url)
    write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 0

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


def test_run_once_parallel_picks_up_new_market_next_cycle(
    tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, parallel_markets=True, dexie_api_base=dexie_mock.base_url)
    write_markets(markets)
    db_path = tmp_path / "state.sqlite"

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 0

    write_markets_two(markets)
    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 0

    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(event_types=["daemon_cycle_summary"], limit=2)
    finally:
        store.close()
    attempted = sorted(int(e["payload"]["markets_attempted"]) for e in events)
    processed = sorted(int(e["payload"]["markets_processed"]) for e in events)
    assert attempted == [1, 2]
    assert processed == [1, 2]


def test_run_once_uses_websocket_capture_when_enabled(
    tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, dexie_api_base=dexie_mock.base_url)
    write_markets(markets)

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://api.coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
        use_websocket_capture=True,
    )
    assert code == 0

    store = SqliteStore(tmp_path / "state.sqlite")
    try:
        events = store.list_recent_audit_events(
            event_types=["coinset_ws_once_started", "coinset_ws_recovery_poll"],
            limit=5,
        )
    finally:
        store.close()
    event_types = {str(event["event_type"]) for event in events}
    assert "coinset_ws_once_started" in event_types
    assert "coinset_ws_recovery_poll" in event_types


def test_daemon_instance_lock_rejects_second_holder(tmp_path: Path) -> None:
    from greenfloor.daemon.main import _acquire_daemon_instance_lock

    state_dir = tmp_path / "state"
    with _acquire_daemon_instance_lock(state_dir=state_dir, mode="loop"):
        with pytest.raises(RuntimeError, match="daemon_already_running"):
            with _acquire_daemon_instance_lock(state_dir=state_dir, mode="once"):
                pass


def test_main_once_exits_with_lock_conflict(
    monkeypatch, tmp_path: Path, capsys, dexie_mock: DexieHttpMock
) -> None:
    import greenfloor.daemon.main as daemon_main_module
    from greenfloor.daemon.main import _acquire_daemon_instance_lock
    from greenfloor.daemon.main import main as daemon_cli_main

    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    write_program(program, home, dexie_api_base=dexie_mock.base_url)
    reset_concurrent_log_handlers(module=daemon_main_module)
    state_dir = tmp_path / "state"
    with _acquire_daemon_instance_lock(state_dir=state_dir, mode="loop"):
        monkeypatch.setattr(
            "sys.argv",
            [
                "greenfloord",
                "--once",
                "--program-config",
                str(program),
                "--state-dir",
                str(state_dir),
            ],
        )
        with pytest.raises(SystemExit) as exc:
            daemon_cli_main()
        assert exc.value.code == 3
        captured = capsys.readouterr()
        assert captured.out == ""
        log_text = (home / "logs" / "debug.log").read_text(encoding="utf-8")
        assert "daemon_lock_conflict" in log_text


def test_run_once_all_markets_fail_exits_non_zero(
    monkeypatch, tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
    monkeypatch.setenv("GREENFLOOR_TEST_FORCE_MARKET_ERROR", "m1")
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, parallel_markets=False, dexie_api_base=dexie_mock.base_url)
    write_markets(markets)
    db_path = tmp_path / "state.sqlite"

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 1

from __future__ import annotations

import threading
from pathlib import Path

import pytest

from greenfloor.daemon.testing import main, run_once
from greenfloor.storage.sqlite import SqliteStore
from tests.helpers.daemon_websocket_fixtures import (
    write_markets,
    write_markets_two,
    write_program,
)
from tests.logging_helpers import reset_concurrent_log_handlers


def test_run_once_parallel_markets_overlap_execution(monkeypatch, tmp_path: Path) -> None:
    from greenfloor.daemon.market_cycle import MarketCycleResult

    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, parallel_markets=True)
    write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    class _FakePriceAdapter:
        async def get_xch_price(self) -> float:
            return 30.0

    class _FakeWalletAdapter:
        pass

    class _FakeDexieAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    class _FakeSplashAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    started: list[str] = []
    started_lock = threading.Lock()
    both_started = threading.Event()

    def _fake_process_single_market(**kwargs):
        market = kwargs["market"]
        with started_lock:
            started.append(str(market.market_id))
            if len(started) == 2:
                both_started.set()
        assert both_started.wait(timeout=1.0)
        return MarketCycleResult()

    monkeypatch.setattr(main, "PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr(main, "WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr(main, "DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr(main, "SplashAdapter", _FakeSplashAdapter)
    monkeypatch.setattr(
        "greenfloor.daemon.cycle_runner.process_single_market_with_store",
        _fake_process_single_market,
    )

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
    assert set(started) == {"m1", "m2"}


def test_run_once_parallel_market_failure_isolated(monkeypatch, tmp_path: Path) -> None:
    from greenfloor.daemon.market_cycle import MarketCycleResult

    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, parallel_markets=True)
    write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    class _FakePriceAdapter:
        async def get_xch_price(self) -> float:
            return 30.0

    class _FakeWalletAdapter:
        pass

    class _FakeDexieAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    class _FakeSplashAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    def _fake_process_single_market(**kwargs):
        market = kwargs["market"]
        if str(market.market_id) == "m1":
            raise RuntimeError("boom")
        return MarketCycleResult(strategy_planned=2, strategy_executed=1)

    monkeypatch.setattr(main, "PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr(main, "WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr(main, "DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr(main, "SplashAdapter", _FakeSplashAdapter)
    monkeypatch.setattr(
        "greenfloor.daemon.cycle_runner.process_single_market_with_store",
        _fake_process_single_market,
    )

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


def test_run_once_sequential_market_failure_isolated(monkeypatch, tmp_path: Path) -> None:
    from greenfloor.daemon.market_cycle import MarketCycleResult

    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, parallel_markets=False)
    write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    class _FakePriceAdapter:
        async def get_xch_price(self) -> float:
            return 30.0

    class _FakeWalletAdapter:
        pass

    class _FakeDexieAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    class _FakeSplashAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    def _fake_process_single_market(**kwargs):
        market = kwargs["market"]
        if str(market.market_id) == "m1":
            raise RuntimeError("boom-sequential")
        return MarketCycleResult(strategy_planned=2, strategy_executed=1)

    monkeypatch.setattr(main, "PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr(main, "WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr(main, "DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr(main, "SplashAdapter", _FakeSplashAdapter)
    monkeypatch.setattr(
        "greenfloor.daemon.cycle_runner.process_single_market", _fake_process_single_market
    )

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


def test_run_once_parallel_picks_up_new_market_next_cycle(monkeypatch, tmp_path: Path) -> None:
    from greenfloor.daemon.market_cycle import MarketCycleResult

    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home, parallel_markets=True)
    write_markets(markets)  # first cycle has one enabled market
    db_path = tmp_path / "state.sqlite"

    class _FakePriceAdapter:
        async def get_xch_price(self) -> float:
            return 30.0

    class _FakeWalletAdapter:
        pass

    class _FakeDexieAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    class _FakeSplashAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    sequential_seen: list[str] = []
    parallel_seen: list[str] = []

    def _fake_process_single_market(**kwargs):
        market = kwargs["market"]
        sequential_seen.append(str(market.market_id))
        return MarketCycleResult()

    def _fake_process_single_market_with_store(**kwargs):
        market = kwargs["market"]
        parallel_seen.append(str(market.market_id))
        return MarketCycleResult()

    monkeypatch.setattr(main, "PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr(main, "WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr(main, "DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr(main, "SplashAdapter", _FakeSplashAdapter)
    monkeypatch.setattr(
        "greenfloor.daemon.cycle_runner.process_single_market", _fake_process_single_market
    )
    monkeypatch.setattr(
        "greenfloor.daemon.cycle_runner.process_single_market_with_store",
        _fake_process_single_market_with_store,
    )

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
    assert sequential_seen == ["m1"]
    assert parallel_seen == []
    cycle1_parallel_count = len(parallel_seen)

    write_markets_two(markets)  # add a new enabled market while daemon keeps running
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
    assert len(parallel_seen) == cycle1_parallel_count + 2
    assert set(parallel_seen[cycle1_parallel_count:]) == {"m1", "m2"}

    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(event_types=["daemon_cycle_summary"], limit=2)
    finally:
        store.close()
    attempted = sorted(int(e["payload"]["markets_attempted"]) for e in events)
    processed = sorted(int(e["payload"]["markets_processed"]) for e in events)
    assert attempted == [1, 2]
    assert processed == [1, 2]


def test_run_once_uses_websocket_capture_when_enabled(monkeypatch, tmp_path: Path) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home)
    write_markets(markets)

    class _FakePriceAdapter:
        async def get_xch_price(self) -> float:
            return 30.0

    class _FakeWalletAdapter:
        def list_asset_coins_base_units(self, **_kwargs) -> list[int]:
            return []

        def execute_coin_ops(self, **_kwargs) -> dict:
            return {"dry_run": False, "planned_count": 0, "executed_count": 0, "items": []}

    class _FakeDexieAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

        def get_offers(self, _offered: str, _requested: str) -> list[dict]:
            return []

    capture_calls = {"n": 0}

    def _fake_capture(**_kwargs) -> None:
        capture_calls["n"] += 1

    monkeypatch.setattr(main, "PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr(main, "WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr(main, "DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr(main, "_run_coinset_signal_capture_once", _fake_capture)

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
        use_websocket_capture=True,
    )
    assert code == 0
    assert capture_calls["n"] == 1


def test_daemon_instance_lock_rejects_second_holder(tmp_path: Path) -> None:
    from greenfloor.daemon.main import _acquire_daemon_instance_lock

    state_dir = tmp_path / "state"
    with _acquire_daemon_instance_lock(state_dir=state_dir, mode="loop"):
        with pytest.raises(RuntimeError, match="daemon_already_running"):
            with _acquire_daemon_instance_lock(state_dir=state_dir, mode="once"):
                pass


def test_main_once_exits_with_lock_conflict(monkeypatch, tmp_path: Path, capsys) -> None:
    import greenfloor.daemon.main as daemon_main_module
    from greenfloor.daemon.main import _acquire_daemon_instance_lock
    from greenfloor.daemon.main import main as daemon_cli_main

    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    write_program(program, home)
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

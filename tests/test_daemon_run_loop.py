from __future__ import annotations

import threading
from pathlib import Path

import pytest

from greenfloor.daemon.testing import main, run_loop, run_once
from greenfloor.storage.sqlite import SqliteStore
from tests.helpers.daemon_websocket_fixtures import (
    write_markets,
    write_markets_two,
    write_program,
    write_program_without_log_level,
)
from tests.logging_helpers import reset_concurrent_log_handlers

def test_run_loop_starts_coinset_websocket_client(monkeypatch, tmp_path: Path) -> None:
    from greenfloor.daemon.testing import main as daemon_mod

    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home)
    write_markets(markets)
    reset_concurrent_log_handlers(module=daemon_mod)

    calls: dict[str, int] = {"start": 0, "stop": 0, "run_once": 0}
    run_once_kwargs: dict[str, object] = {}

    class _FakeWsClient:
        def __init__(self, **_kwargs) -> None:
            pass

        def start(self) -> None:
            calls["start"] += 1

        def stop(self, **_kwargs) -> None:
            calls["stop"] += 1

    def _fake_run_once(**kwargs):
        calls["run_once"] += 1
        run_once_kwargs.update(kwargs)
        raise KeyboardInterrupt

    monkeypatch.setattr(main, "CoinsetWebsocketClient", _FakeWsClient)
    monkeypatch.setattr(main, "run_once", _fake_run_once)

    code = run_loop(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=home / "state",
    )

    assert code == 0
    assert calls["start"] == 1
    assert calls["stop"] == 1
    assert calls["run_once"] == 1
    assert run_once_kwargs["poll_coinset_mempool"] is False
    log_text = (home / "logs" / "debug.log").read_text(encoding="utf-8")
    assert "daemon_starting mode=loop" in log_text
    assert "daemon_stopped mode=loop" in log_text


def test_run_loop_refreshes_log_level_without_restart(monkeypatch, tmp_path: Path) -> None:
    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home)
    write_markets(markets)

    calls: dict[str, int] = {"run_once": 0}
    seen_levels: list[str] = []

    class _FakeWsClient:
        def __init__(self, **_kwargs) -> None:
            pass

        def start(self) -> None:
            return

        def stop(self, **_kwargs) -> None:
            return

    def _fake_initialize(home_dir: str, *, log_level: str | None) -> None:
        _ = home_dir
        seen_levels.append(str(log_level or ""))

    def _fake_run_once(**_kwargs):
        calls["run_once"] += 1
        if calls["run_once"] == 1:
            text = program.read_text(encoding="utf-8")
            program.write_text(
                text.replace("  log_level: INFO", "  log_level: WARNING"), encoding="utf-8"
            )
            return 0
        raise KeyboardInterrupt

    monkeypatch.setattr(main, "CoinsetWebsocketClient", _FakeWsClient)
    monkeypatch.setattr(main, "_initialize_daemon_file_logging", _fake_initialize)
    monkeypatch.setattr(main, "run_once", _fake_run_once)
    monkeypatch.setattr(main.time, "sleep", lambda _seconds: None)

    code = run_loop(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=home / "state",
    )

    assert code == 0
    assert calls["run_once"] == 2
    assert seen_levels[:3] == ["INFO", "INFO", "WARNING"]


def test_run_loop_logs_when_missing_log_level_is_auto_healed(monkeypatch, tmp_path: Path) -> None:
    from greenfloor.daemon.testing import main as daemon_mod

    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_without_log_level(program, home)
    write_markets(markets)
    reset_concurrent_log_handlers(module=daemon_mod)

    class _FakeWsClient:
        def __init__(self, **_kwargs) -> None:
            pass

        def start(self) -> None:
            return

        def stop(self, **_kwargs) -> None:
            return

    def _fake_run_once(**_kwargs):
        raise KeyboardInterrupt

    monkeypatch.setattr(main, "CoinsetWebsocketClient", _FakeWsClient)
    monkeypatch.setattr(main, "run_once", _fake_run_once)

    code = run_loop(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=home / "state",
    )
    assert code == 0
    assert "log_level: INFO" in program.read_text(encoding="utf-8")
    log_text = (home / "logs" / "debug.log").read_text(encoding="utf-8")
    assert "program config missing app.log_level; wrote default INFO" in log_text


def test_run_loop_orders_reload_marker_log_sleep_then_reload(monkeypatch, tmp_path: Path) -> None:
    from greenfloor.daemon.testing import main as daemon_mod

    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home)
    write_markets(markets)

    sequence: list[str] = []
    real_load_program_config = daemon_mod.load_program_config
    load_calls = {"count": 0}

    class _FakeWsClient:
        def __init__(self, **_kwargs) -> None:
            pass

        def start(self) -> None:
            return

        def stop(self, **_kwargs) -> None:
            return

    def _fake_load_program_config(path: Path):
        load_calls["count"] += 1
        sequence.append(f"load_program:{load_calls['count']}")
        if load_calls["count"] == 1:
            return real_load_program_config(path)
        raise KeyboardInterrupt

    def _fake_run_once(**_kwargs):
        sequence.append("run_once")
        return 0

    def _fake_consume_reload_marker(_state_dir: Path) -> bool:
        sequence.append("consume_reload_marker")
        return True

    def _fake_log_daemon_event(*, level: int, payload: dict[str, object]) -> None:
        _ = level
        sequence.append(f"log_event:{payload.get('event')}")

    def _fake_sleep(_seconds: float) -> None:
        sequence.append("sleep")

    monkeypatch.setattr(main, "CoinsetWebsocketClient", _FakeWsClient)
    monkeypatch.setattr(main, "load_program_config", _fake_load_program_config)
    monkeypatch.setattr(main, "run_once", _fake_run_once)
    monkeypatch.setattr(
        "greenfloor.daemon.main._consume_reload_marker", _fake_consume_reload_marker
    )
    monkeypatch.setattr(main, "_log_daemon_event", _fake_log_daemon_event)
    monkeypatch.setattr(main.time, "sleep", _fake_sleep)

    code = run_loop(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=home / "state",
    )

    assert code == 0
    assert sequence[0] == "load_program:1"
    assert "run_once" in sequence
    assert "consume_reload_marker" in sequence
    assert "log_event:config_reloaded" in sequence
    assert "sleep" in sequence
    assert "load_program:2" in sequence
    assert sequence.index("run_once") < sequence.index("consume_reload_marker")
    assert sequence.index("consume_reload_marker") < sequence.index("log_event:config_reloaded")
    assert sequence.index("log_event:config_reloaded") < sequence.index("sleep")
    assert sequence.index("sleep") < sequence.index("load_program:2")


def test_run_loop_websocket_callbacks_use_callback_thread_store(
    monkeypatch, tmp_path: Path
) -> None:
    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home)
    write_markets(markets)

    ws_errors: list[Exception] = []

    class _ThreadBoundStore:
        def __init__(self, _db_path: str) -> None:
            self._thread_id = threading.get_ident()

        def _assert_thread(self) -> None:
            if threading.get_ident() != self._thread_id:
                raise AssertionError("cross_thread_store_use")

        def observe_mempool_tx_ids(self, _tx_ids) -> int:
            self._assert_thread()
            return 1

        def confirm_tx_ids(self, _tx_ids) -> int:
            self._assert_thread()
            return 1

        def add_audit_event(self, _event_type: str, _payload: dict) -> None:
            self._assert_thread()

        def close(self) -> None:
            return

    class _FakeWsClient:
        def __init__(self, **kwargs) -> None:
            self._kwargs = kwargs

        def start(self) -> None:
            def _invoke_callbacks() -> None:
                try:
                    self._kwargs["on_audit_event"]("coinset_ws_connected", {"ok": True})
                    self._kwargs["on_mempool_tx_ids"](["a" * 64])
                    self._kwargs["on_confirmed_tx_ids"](["b" * 64])
                except Exception as exc:  # pragma: no cover - assertion path
                    ws_errors.append(exc)

            t = threading.Thread(target=_invoke_callbacks)
            t.start()
            t.join()

        def stop(self, **_kwargs) -> None:
            return

    def _fake_run_once(**_kwargs):
        raise KeyboardInterrupt

    monkeypatch.setattr(main, "SqliteStore", _ThreadBoundStore)
    monkeypatch.setattr(main, "CoinsetWebsocketClient", _FakeWsClient)
    monkeypatch.setattr(main, "run_once", _fake_run_once)

    code = run_loop(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://api.coinset.org",
        state_dir=home / "state",
    )

    assert code == 0
    assert ws_errors == []



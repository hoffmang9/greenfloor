from __future__ import annotations

import logging
from pathlib import Path

from greenfloor.daemon.testing import main, run_loop
from tests.helpers.daemon_websocket_fixtures import (
    write_markets,
    write_program,
    write_program_without_log_level,
)
from tests.logging_helpers import reset_concurrent_log_handlers


def _patch_engine(monkeypatch, *, ws_client_factory, init_logging=None, warn_healed=None):
    class _FakeEngine:
        def start_coinset_websocket_loop(self, *_args, **_kwargs):
            return ws_client_factory()

        def initialize_daemon_file_logging(self, home_dir, log_level):
            if init_logging is not None:
                init_logging(home_dir, log_level=log_level)

        def warn_if_daemon_log_level_auto_healed(self, missing, path):
            if warn_healed is not None:
                warn_healed(missing, path)

    monkeypatch.setattr(main, "_engine", lambda: _FakeEngine())


def test_run_loop_starts_coinset_websocket_client(monkeypatch, tmp_path: Path, caplog) -> None:
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
        def __init__(self) -> None:
            pass

        def stop(self) -> None:
            calls["stop"] += 1

    def _ws_factory():
        calls["start"] += 1
        return _FakeWsClient()

    def _fake_run_once(**kwargs):
        calls["run_once"] += 1
        run_once_kwargs.update(kwargs)
        raise KeyboardInterrupt

    _patch_engine(monkeypatch, ws_client_factory=_ws_factory)
    monkeypatch.setattr(main, "run_once", _fake_run_once)

    with caplog.at_level(logging.INFO, logger="greenfloor.daemon"):
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
    assert "daemon_starting mode=loop" in caplog.text
    assert "daemon_stopped mode=loop" in caplog.text


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
        def stop(self) -> None:
            return

    def _fake_initialize(home_dir, log_level):
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

    _patch_engine(
        monkeypatch,
        ws_client_factory=_FakeWsClient,
        init_logging=_fake_initialize,
    )
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
        def stop(self) -> None:
            return

    def _fake_run_once(**_kwargs):
        raise KeyboardInterrupt

    _patch_engine(monkeypatch, ws_client_factory=_FakeWsClient)
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
        def stop(self) -> None:
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

    _patch_engine(monkeypatch, ws_client_factory=_FakeWsClient)
    monkeypatch.setattr(main, "load_program_config", _fake_load_program_config)
    monkeypatch.setattr(main, "run_once", _fake_run_once)
    monkeypatch.setattr(
        "greenfloor.daemon.cycle_runner.consume_reload_marker", _fake_consume_reload_marker
    )
    monkeypatch.setattr(main, "log_daemon_event", _fake_log_daemon_event)
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

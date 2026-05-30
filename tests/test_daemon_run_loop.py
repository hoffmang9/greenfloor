from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock

from greenfloor.daemon.cycle_runner import run_loop


def test_run_loop_delegates_to_engine_and_returns_zero_on_interrupt(
    monkeypatch, tmp_path: Path
) -> None:
    calls: dict[str, int] = {"loop": 0}

    class _FakeLoopRequest:
        def __init__(self, *args, **kwargs) -> None:
            self.args = args
            self.kwargs = kwargs

    def _fake_run_daemon_loop(_request) -> int:
        calls["loop"] += 1
        raise KeyboardInterrupt

    fake_engine = MagicMock()
    fake_engine.DaemonLoopRequest = _FakeLoopRequest
    fake_engine.run_daemon_loop = _fake_run_daemon_loop
    monkeypatch.setattr("greenfloor.daemon.cycle_runner.import_engine", lambda: fake_engine)
    monkeypatch.setattr(
        "greenfloor.daemon.cycle_runner.initialize_daemon_logging",
        lambda **_kwargs: None,
    )
    monkeypatch.setattr(
        "greenfloor.daemon.cycle_runner.load_program_config",
        lambda _path: MagicMock(home_dir=str(tmp_path / "home")),
    )

    code = run_loop(
        program_path=tmp_path / "program.yaml",
        markets_path=tmp_path / "markets.yaml",
        allowed_keys={"key-1"},
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=tmp_path / "state",
    )

    assert code == 0
    assert calls["loop"] == 1

from __future__ import annotations

import importlib
from pathlib import Path
from unittest.mock import MagicMock

from greenfloor.daemon.testing.main import run_loop


def test_run_loop_delegates_to_engine_and_returns_zero_on_interrupt(
    monkeypatch, tmp_path: Path
) -> None:
    calls: dict[str, int] = {"loop": 0}

    def _fake_run_daemon_loop(_request: dict) -> int:
        calls["loop"] += 1
        raise KeyboardInterrupt

    fake_engine = MagicMock()
    fake_engine.run_daemon_loop = _fake_run_daemon_loop
    testing_main = importlib.import_module("greenfloor.daemon.testing.main")
    monkeypatch.setattr(testing_main, "import_engine", lambda: fake_engine)
    monkeypatch.setattr(
        testing_main,
        "require_engine_method",
        lambda _engine, name, **kwargs: getattr(fake_engine, name),
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

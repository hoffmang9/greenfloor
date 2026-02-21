from __future__ import annotations

from pathlib import Path

from greenfloor.cli.manager import _default_markets_config_path, _default_program_config_path


def test_default_paths_fall_back_to_repo_config_when_home_missing(monkeypatch) -> None:
    monkeypatch.setattr(Path, "exists", lambda _self: False)
    assert _default_program_config_path() == "config/program.yaml"
    assert _default_markets_config_path() == "config/markets.yaml"


def test_default_paths_prefer_home_when_present(monkeypatch) -> None:
    def _fake_exists(self: Path) -> bool:
        return str(self).endswith("/.greenfloor/config/program.yaml") or str(self).endswith(
            "/.greenfloor/config/markets.yaml"
        )

    monkeypatch.setattr(Path, "exists", _fake_exists)
    assert _default_program_config_path().endswith("/.greenfloor/config/program.yaml")
    assert _default_markets_config_path().endswith("/.greenfloor/config/markets.yaml")

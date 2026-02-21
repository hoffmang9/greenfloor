import shutil
from pathlib import Path

from greenfloor.cli.manager import _config_history_list, _config_history_revert
from greenfloor.config.editor import write_yaml_versioned
from greenfloor.config.io import load_yaml


def test_manager_config_history_list_and_revert(tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    shutil.copyfile("config/program.yaml", program)
    markets = tmp_path / "markets.yaml"
    write_yaml_versioned(
        path=markets,
        data={"markets": [{"id": "m1", "enabled": True}]},
        actor="test",
        reason="first",
    )
    second = write_yaml_versioned(
        path=markets,
        data={"markets": [{"id": "m2", "enabled": True}]},
        actor="test",
        reason="second",
    )

    code = _config_history_list(markets)
    assert code == 0
    listed = capsys.readouterr().out
    assert '"history"' in listed

    backup_path = Path(str(second["backup_path"]))
    code = _config_history_revert(
        program_path=program,
        config_path=markets,
        backup_path=backup_path,
        latest=False,
        state_db=str(tmp_path / "state.sqlite"),
        reload=True,
        state_dir=tmp_path / "state",
        yes=True,
    )
    assert code == 0
    reverted = load_yaml(markets)
    assert reverted["markets"][0]["id"] == "m1"
    assert (tmp_path / "state" / "reload_request.json").exists()


def test_manager_config_history_revert_latest(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    shutil.copyfile("config/program.yaml", program)
    config_path = tmp_path / "program_local.yaml"
    write_yaml_versioned(path=config_path, data={"v": 1}, actor="test", reason="first")
    write_yaml_versioned(path=config_path, data={"v": 2}, actor="test", reason="second")
    code = _config_history_revert(
        program_path=program,
        config_path=config_path,
        backup_path=None,
        latest=True,
        state_db=str(tmp_path / "state.sqlite"),
        reload=False,
        state_dir=tmp_path / "state",
        yes=True,
    )
    assert code == 0
    loaded = load_yaml(config_path)
    assert loaded["v"] == 1


def test_manager_config_history_revert_cancelled_by_default(
    tmp_path: Path, monkeypatch, capsys
) -> None:
    program = tmp_path / "program.yaml"
    shutil.copyfile("config/program.yaml", program)
    config_path = tmp_path / "program_local.yaml"
    write_yaml_versioned(path=config_path, data={"v": 1}, actor="test", reason="first")
    second = write_yaml_versioned(path=config_path, data={"v": 2}, actor="test", reason="second")

    monkeypatch.setattr("builtins.input", lambda _prompt: "n")
    code = _config_history_revert(
        program_path=program,
        config_path=config_path,
        backup_path=Path(str(second["backup_path"])),
        latest=False,
        state_db=str(tmp_path / "state.sqlite"),
        reload=False,
        state_dir=tmp_path / "state",
        yes=False,
    )
    assert code == 0
    out = capsys.readouterr().out
    assert '"cancelled": true' in out.lower()
    loaded = load_yaml(config_path)
    assert loaded["v"] == 2

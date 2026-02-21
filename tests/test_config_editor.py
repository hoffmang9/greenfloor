from pathlib import Path

from greenfloor.config.editor import (
    latest_yaml_backup_path,
    list_yaml_history,
    revert_yaml_from_backup,
    write_yaml_versioned,
)
from greenfloor.config.io import load_yaml


def test_write_yaml_versioned_creates_file(tmp_path: Path) -> None:
    target = tmp_path / "markets.yaml"
    result = write_yaml_versioned(
        path=target,
        data={"markets": [{"id": "m1"}]},
        actor="test",
        reason="initial_write",
    )
    assert target.exists()
    assert result["new_checksum"] is not None
    assert result["backup_path"] is None
    loaded = load_yaml(target)
    assert loaded["markets"][0]["id"] == "m1"


def test_write_yaml_versioned_creates_backup_on_update(tmp_path: Path) -> None:
    target = tmp_path / "program.yaml"
    write_yaml_versioned(
        path=target,
        data={"app": {"network": "mainnet"}},
        actor="test",
        reason="first",
    )
    result = write_yaml_versioned(
        path=target,
        data={"app": {"network": "testnet11"}},
        actor="test",
        reason="second",
    )
    assert result["backup_path"] is not None
    assert result["backup_meta_path"] is not None
    assert Path(str(result["backup_path"])).exists()
    assert Path(str(result["backup_meta_path"])).exists()


def test_list_yaml_history_and_revert(tmp_path: Path) -> None:
    target = tmp_path / "program.yaml"
    write_yaml_versioned(
        path=target,
        data={"app": {"network": "mainnet"}},
        actor="test",
        reason="first",
    )
    second = write_yaml_versioned(
        path=target,
        data={"app": {"network": "testnet11"}},
        actor="test",
        reason="second",
    )
    history = list_yaml_history(target)
    assert len(history) >= 1
    backup_path = Path(str(second["backup_path"]))
    revert_yaml_from_backup(
        path=target,
        backup_path=backup_path,
        actor="test",
        reason="revert",
    )
    loaded = load_yaml(target)
    assert loaded["app"]["network"] == "mainnet"


def test_latest_yaml_backup_path(tmp_path: Path) -> None:
    target = tmp_path / "config.yaml"
    write_yaml_versioned(path=target, data={"x": 1}, actor="test", reason="a")
    second = write_yaml_versioned(path=target, data={"x": 2}, actor="test", reason="b")
    latest = latest_yaml_backup_path(target)
    assert latest is not None
    assert latest == Path(str(second["backup_path"]))


def test_revert_rejects_mismatched_backup_namespace(tmp_path: Path) -> None:
    target_a = tmp_path / "a.yaml"
    target_b = tmp_path / "b.yaml"
    write_yaml_versioned(path=target_a, data={"v": "a1"}, actor="test", reason="a1")
    second_a = write_yaml_versioned(path=target_a, data={"v": "a2"}, actor="test", reason="a2")
    write_yaml_versioned(path=target_b, data={"v": "b1"}, actor="test", reason="b1")

    bad_backup = Path(str(second_a["backup_path"]))
    try:
        revert_yaml_from_backup(path=target_b, backup_path=bad_backup, actor="test", reason="bad")
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert "history namespace" in str(exc)

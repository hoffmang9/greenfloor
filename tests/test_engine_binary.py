from __future__ import annotations

from pathlib import Path

import pytest

from greenfloor.engine_binary import (
    GreenfloorEngineBinaryError,
    repo_root,
    resolve_greenfloor_engine_binary,
    resolve_greenfloor_manager_binary,
    resolve_greenfloord_binary,
)


def test_repo_root_points_at_project_root() -> None:
    root = repo_root()
    assert (root / "greenfloor-engine" / "Cargo.toml").is_file()
    assert (root / "pyproject.toml").is_file()


def test_resolve_engine_binary_from_target_debug(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    fake_bin = tmp_path / "greenfloor-engine"
    fake_bin.write_text("#!/bin/sh\n", encoding="utf-8")
    fake_bin.chmod(0o755)
    monkeypatch.setenv("GREENFLOOR_ENGINE_BIN", str(fake_bin))
    assert resolve_greenfloor_engine_binary(build_if_missing=False) == fake_bin


def test_resolve_manager_binary_from_path_override(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    fake_bin = tmp_path / "greenfloor-manager"
    fake_bin.write_text("#!/bin/sh\n", encoding="utf-8")
    fake_bin.chmod(0o755)
    monkeypatch.setenv("GREENFLOOR_MANAGER_BIN", str(fake_bin))
    assert resolve_greenfloor_manager_binary(build_if_missing=False) == fake_bin


def test_resolve_daemon_binary_raises_for_invalid_override(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    missing = tmp_path / "missing-greenfloord"
    monkeypatch.setenv("GREENFLOOR_DAEMON_BIN", str(missing))
    with pytest.raises(GreenfloorEngineBinaryError):
        resolve_greenfloord_binary(build_if_missing=False)

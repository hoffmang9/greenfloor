"""Resolve built greenfloor-engine binaries for scripts and tests."""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

_ENGINE_BIN = "greenfloor-engine"
_MANAGER_BIN = "greenfloor-manager"
_DAEMON_BIN = "greenfloord"
_ALL_BINS = (_ENGINE_BIN, _MANAGER_BIN, _DAEMON_BIN)


class GreenfloorEngineBinaryError(FileNotFoundError):
    """Raised when a GreenFloor native binary cannot be located."""


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def _candidate_paths(binary_name: str) -> tuple[Path, ...]:
    root = repo_root()
    return (
        root / "target" / "debug" / binary_name,
        root / "target" / "release" / binary_name,
        root / "greenfloor-engine" / "target" / "debug" / binary_name,
        root / "greenfloor-engine" / "target" / "release" / binary_name,
    )


def _build_engine_binaries() -> None:
    root = repo_root()
    manifest = root / "greenfloor-engine" / "Cargo.toml"
    if not manifest.is_file():
        raise GreenfloorEngineBinaryError(
            "greenfloor-engine Cargo.toml not found; cannot build binaries"
        )
    env = os.environ.copy()
    env.setdefault("CARGO_TARGET_DIR", str(root / "target"))
    cmd = ["cargo", "build", "--manifest-path", str(manifest)]
    for binary_name in _ALL_BINS:
        cmd.extend(["--bin", binary_name])
    subprocess.run(cmd, check=True, cwd=root, env=env)


def _resolve_binary(
    binary_name: str,
    *,
    env_var: str,
    build_if_missing: bool,
) -> Path:
    override = os.environ.get(env_var, "").strip()
    if override:
        path = Path(override).expanduser()
        if not path.is_file():
            raise GreenfloorEngineBinaryError(f"{env_var} is not an executable file: {path}")
        return path

    discovered = shutil.which(binary_name)
    if discovered:
        return Path(discovered)

    for candidate in _candidate_paths(binary_name):
        if candidate.is_file():
            return candidate

    if build_if_missing:
        _build_engine_binaries()
        return _resolve_binary(binary_name, env_var=env_var, build_if_missing=False)

    raise GreenfloorEngineBinaryError(
        f"{binary_name} binary not found; build with "
        "'cargo build --manifest-path greenfloor-engine/Cargo.toml "
        "--bin greenfloor-engine --bin greenfloor-manager --bin greenfloord' "
        f"or set {env_var}"
    )


def resolve_greenfloor_engine_binary(*, build_if_missing: bool = True) -> Path:
    return _resolve_binary(
        _ENGINE_BIN,
        env_var="GREENFLOOR_ENGINE_BIN",
        build_if_missing=build_if_missing,
    )


def resolve_greenfloor_manager_binary(*, build_if_missing: bool = True) -> Path:
    return _resolve_binary(
        _MANAGER_BIN,
        env_var="GREENFLOOR_MANAGER_BIN",
        build_if_missing=build_if_missing,
    )


def resolve_greenfloord_binary(*, build_if_missing: bool = True) -> Path:
    return _resolve_binary(
        _DAEMON_BIN,
        env_var="GREENFLOOR_DAEMON_BIN",
        build_if_missing=build_if_missing,
    )

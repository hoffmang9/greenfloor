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


def resolve_greenfloor_engine_binary(*, build_if_missing: bool = True) -> Path:
    override = os.environ.get("GREENFLOOR_ENGINE_BIN", "").strip()
    if override:
        path = Path(override).expanduser()
        if not path.is_file():
            raise GreenfloorEngineBinaryError(
                f"GREENFLOOR_ENGINE_BIN is not an executable file: {path}"
            )
        return path

    discovered = shutil.which(_ENGINE_BIN)
    if discovered:
        return Path(discovered)

    for candidate in _candidate_paths(_ENGINE_BIN):
        if candidate.is_file():
            return candidate

    if build_if_missing:
        _build_engine_binaries()
        return resolve_greenfloor_engine_binary(build_if_missing=False)

    raise GreenfloorEngineBinaryError(
        "greenfloor-engine binary not found; build with "
        "'cargo build --manifest-path greenfloor-engine/Cargo.toml "
        "--bin greenfloor-engine --bin greenfloor-manager --bin greenfloord' "
        "or set GREENFLOOR_ENGINE_BIN"
    )


def resolve_greenfloord_binary(*, build_if_missing: bool = True) -> Path:
    override = os.environ.get("GREENFLOOR_DAEMON_BIN", "").strip()
    if override:
        path = Path(override).expanduser()
        if not path.is_file():
            raise GreenfloorEngineBinaryError(
                f"GREENFLOOR_DAEMON_BIN is not an executable file: {path}"
            )
        return path

    discovered = shutil.which(_DAEMON_BIN)
    if discovered:
        return Path(discovered)

    for candidate in _candidate_paths(_DAEMON_BIN):
        if candidate.is_file():
            return candidate

    if build_if_missing:
        _build_engine_binaries()
        return resolve_greenfloord_binary(build_if_missing=False)

    raise GreenfloorEngineBinaryError(
        "greenfloord binary not found; build with "
        "'cargo build --manifest-path greenfloor-engine/Cargo.toml "
        "--bin greenfloor-engine --bin greenfloor-manager --bin greenfloord' "
        "or set GREENFLOOR_DAEMON_BIN"
    )

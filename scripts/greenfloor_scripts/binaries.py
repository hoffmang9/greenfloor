"""Resolve built greenfloor-engine binaries for scripts and tests."""

from __future__ import annotations

import functools
import json
import os
import shutil
import subprocess
from pathlib import Path

_ENGINE_BIN = "greenfloor-engine"
_MANAGER_BIN = "greenfloor-manager"
_DAEMON_BIN = "greenfloord"
_ALL_BINS = (_ENGINE_BIN, _MANAGER_BIN, _DAEMON_BIN)
_MANIFEST_REL = Path("greenfloor-engine") / "Cargo.toml"


class GreenfloorEngineBinaryError(FileNotFoundError):
    """Raised when a GreenFloor native binary cannot be located."""


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _engine_manifest() -> Path:
    return repo_root() / _MANIFEST_REL


@functools.lru_cache(maxsize=1)
def cargo_target_directory() -> Path:
    """Return Cargo's resolved target directory (from repo-root ``.cargo/config.toml``)."""
    root = repo_root()
    manifest = _engine_manifest()
    if not manifest.is_file():
        raise GreenfloorEngineBinaryError(
            "greenfloor-engine Cargo.toml not found; cannot resolve Cargo target directory"
        )
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            str(manifest),
            "--format-version",
            "1",
            "--no-deps",
        ],
        check=True,
        cwd=root,
        capture_output=True,
        text=True,
    )
    payload = json.loads(result.stdout)
    target_directory = payload.get("target_directory")
    if not isinstance(target_directory, str) or not target_directory.strip():
        raise GreenfloorEngineBinaryError("cargo metadata did not return a usable target_directory")
    return Path(target_directory)


def _candidate_paths(binary_name: str) -> tuple[Path, ...]:
    target_root = cargo_target_directory()
    return (
        target_root / "debug" / binary_name,
        target_root / "release" / binary_name,
    )


def _build_engine_binaries() -> None:
    root = repo_root()
    manifest = _engine_manifest()
    if not manifest.is_file():
        raise GreenfloorEngineBinaryError(
            "greenfloor-engine Cargo.toml not found; cannot build binaries"
        )
    cmd = ["cargo", "build", "--manifest-path", str(manifest)]
    for binary_name in _ALL_BINS:
        cmd.extend(["--bin", binary_name])
    subprocess.run(cmd, check=True, cwd=root)


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

    for candidate in _candidate_paths(binary_name):
        if candidate.is_file():
            return candidate

    discovered = shutil.which(binary_name)
    if discovered:
        return Path(discovered)

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

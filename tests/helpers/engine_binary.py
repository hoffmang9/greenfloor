"""Resolve built greenfloor-engine binary for integration tests."""

from __future__ import annotations

import subprocess
from pathlib import Path


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def engine_binary_path(*, build_if_missing: bool = True) -> Path:
    root = repo_root()
    target_dir = root / "greenfloor-engine" / "target"
    candidates = (
        root / "greenfloor-engine" / "target" / "debug" / "greenfloor-engine",
        root / "greenfloor-engine" / "target" / "release" / "greenfloor-engine",
    )
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    manifest = root / "greenfloor-engine" / "Cargo.toml"
    if build_if_missing and manifest.is_file():
        target_dir.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            [
                "cargo",
                "build",
                "--manifest-path",
                str(manifest),
                "--target-dir",
                str(target_dir),
            ],
            check=True,
            cwd=root,
        )
        return engine_binary_path(build_if_missing=False)
    raise FileNotFoundError(
        "greenfloor-engine binary not found; build with "
        "'cargo build --manifest-path greenfloor-engine/Cargo.toml'"
    )

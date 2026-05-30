"""Locate the native greenfloor-engine binary (vault/coin-op CLI paths)."""

from __future__ import annotations

import os
import shutil
from pathlib import Path


class GreenfloorEngineBinaryError(RuntimeError):
    """Raised when the greenfloor-engine binary cannot be located."""


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def resolve_greenfloor_engine_binary() -> Path:
    override = os.environ.get("GREENFLOOR_ENGINE_BIN", "").strip()
    if override:
        path = Path(override).expanduser()
        if not path.is_file():
            raise GreenfloorEngineBinaryError(
                f"GREENFLOOR_ENGINE_BIN is not an executable file: {path}"
            )
        return path

    discovered = shutil.which("greenfloor-engine")
    if discovered:
        return Path(discovered)

    root = repo_root()
    for relative in (
        Path("target/release/greenfloor-engine"),
        Path("target/debug/greenfloor-engine"),
        Path("greenfloor-engine/target/release/greenfloor-engine"),
        Path("greenfloor-engine/target/debug/greenfloor-engine"),
    ):
        candidate = root / relative
        if candidate.is_file():
            return candidate

    raise GreenfloorEngineBinaryError(
        "greenfloor-engine binary not found; build with "
        "'cargo build --manifest-path greenfloor-engine/Cargo.toml' or set GREENFLOOR_ENGINE_BIN"
    )

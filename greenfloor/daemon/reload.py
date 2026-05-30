"""Reload marker helpers backed by the Rust daemon engine."""

from __future__ import annotations

from pathlib import Path

from greenfloor.core.engine_bridge import import_engine, require_engine_method

__all__ = ["consume_reload_marker"]


def consume_reload_marker(state_dir: Path) -> bool:
    fn = require_engine_method(
        import_engine(),
        "consume_reload_marker",
        missing="daemon reload marker",
    )
    return bool(fn(state_dir))

"""Shared daemon engine logging initialization via PyO3."""

from __future__ import annotations

from pathlib import Path

from greenfloor.core.engine_bridge import import_engine, require_engine_method


def _engine():
    return import_engine()


def initialize_daemon_logging(*, program, program_path: Path) -> None:
    init_logging = require_engine_method(
        _engine(),
        "initialize_daemon_file_logging",
        missing="daemon logging",
    )
    warn_healed = require_engine_method(
        _engine(),
        "warn_if_daemon_log_level_auto_healed",
        missing="daemon logging heal warning",
    )
    init_logging(program.home_dir, getattr(program, "app_log_level", "INFO"))
    warn_healed(
        bool(getattr(program, "app_log_level_was_missing", False)),
        str(program_path),
    )

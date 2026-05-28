"""Daemon CLI and loop shared bootstrap (logging init, reload events)."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from greenfloor.daemon.market_logging import _daemon_logger
from greenfloor.logging_setup import (
    initialize_service_file_logging,
    warn_if_log_level_auto_healed,
)

_DAEMON_SERVICE_NAME = "daemon"


def initialize_daemon_file_logging(home_dir: str, *, log_level: str | None) -> None:
    initialize_service_file_logging(
        service_name=_DAEMON_SERVICE_NAME,
        home_dir=home_dir,
        log_level=log_level,
        service_logger=_daemon_logger,
        allow_reinit_level=True,
    )


def warn_if_daemon_log_level_auto_healed(*, program, program_path: Path) -> None:
    warn_if_log_level_auto_healed(
        program_obj=program, program_path=program_path, logger=_daemon_logger
    )


def log_daemon_event(*, level: int, payload: dict[str, Any]) -> None:
    _daemon_logger.log(level, "daemon_event %s", json.dumps(payload, sort_keys=True))

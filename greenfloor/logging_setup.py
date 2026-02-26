from __future__ import annotations

import logging
import os
from pathlib import Path

from concurrent_log_handler import ConcurrentRotatingFileHandler

DEFAULT_LOG_LEVEL_NAME = "INFO"
ALLOWED_LOG_LEVELS = frozenset({"CRITICAL", "ERROR", "WARNING", "INFO", "DEBUG", "NOTSET"})
DEFAULT_LOG_DATE_FORMAT = "%Y-%m-%dT%H:%M:%S"
DEFAULT_LOG_FILE = "logs/debug.log"
DEFAULT_LOG_MAX_FILES_ROTATION = 4
DEFAULT_LOG_MAX_BYTES_ROTATION = 25 * 1024 * 1024


def normalize_log_level_name(log_level: str | None) -> str:
    normalized = str(log_level or "").strip().upper()
    if normalized not in ALLOWED_LOG_LEVELS:
        return DEFAULT_LOG_LEVEL_NAME
    return normalized


def coerce_log_level(log_level: str | None) -> int:
    return cast_log_level(normalize_log_level_name(log_level))


def cast_log_level(level_name: str) -> int:
    level = getattr(logging, level_name, None)
    if not isinstance(level, int):
        return logging.INFO
    return level


def create_rotating_file_handler(*, service_name: str, home_dir: str | Path) -> ConcurrentRotatingFileHandler:
    log_path = (Path(home_dir).expanduser() / DEFAULT_LOG_FILE).resolve()
    log_path.parent.mkdir(parents=True, exist_ok=True)
    file_name_length = 33 - len(service_name)
    formatter = logging.Formatter(
        fmt=(
            f"%(asctime)s.%(msecs)03d {service_name} %(name)-{file_name_length}s: "
            f"%(levelname)-8s %(message)s"
        ),
        datefmt=DEFAULT_LOG_DATE_FORMAT,
    )
    handler = ConcurrentRotatingFileHandler(
        os.fspath(log_path),
        "a",
        maxBytes=DEFAULT_LOG_MAX_BYTES_ROTATION,
        backupCount=DEFAULT_LOG_MAX_FILES_ROTATION,
        use_gzip=False,
    )
    handler.setFormatter(formatter)
    return handler


def apply_level_to_root(*, effective_level: int, logger: logging.Logger, handler: logging.Handler | None) -> None:
    root_logger = logging.getLogger()
    if handler is not None:
        handler.setLevel(effective_level)
    for existing in root_logger.handlers:
        existing.setLevel(effective_level)
    root_logger.setLevel(effective_level)
    logger.setLevel(effective_level)

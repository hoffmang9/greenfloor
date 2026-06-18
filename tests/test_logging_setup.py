from __future__ import annotations

import logging

from greenfloor.logging_setup import (
    ALLOWED_LOG_LEVELS,
    cast_log_level,
    coerce_log_level,
    create_rotating_file_handler,
    initialize_service_file_logging,
    normalize_log_level_name,
)
from tests.logging_helpers import reset_concurrent_log_handlers


def test_normalize_log_level_name_valid_levels() -> None:
    for level in ALLOWED_LOG_LEVELS:
        assert normalize_log_level_name(level) == level
        assert normalize_log_level_name(level.lower()) == level


def test_normalize_log_level_name_defaults_invalid() -> None:
    assert normalize_log_level_name("VERBOSE") == "INFO"
    assert normalize_log_level_name("") == "INFO"
    assert normalize_log_level_name(None) == "INFO"


def test_cast_log_level_known_levels() -> None:
    assert cast_log_level("DEBUG") == logging.DEBUG
    assert cast_log_level("INFO") == logging.INFO
    assert cast_log_level("WARNING") == logging.WARNING
    assert cast_log_level("ERROR") == logging.ERROR
    assert cast_log_level("CRITICAL") == logging.CRITICAL


def test_cast_log_level_unknown_returns_info() -> None:
    assert cast_log_level("BOGUS") == logging.INFO


def test_coerce_log_level_normalizes_then_casts() -> None:
    assert coerce_log_level("debug") == logging.DEBUG
    assert coerce_log_level("VERBOSE") == logging.INFO
    assert coerce_log_level(None) == logging.INFO


def test_create_rotating_file_handler_creates_log_dir(tmp_path) -> None:
    handler = create_rotating_file_handler(service_name="test", home_dir=str(tmp_path))
    assert handler is not None
    log_dir = tmp_path / "logs"
    assert log_dir.exists()
    handler.close()


def test_initialize_service_file_logging_reuses_single_process_handler(tmp_path) -> None:
    import greenfloor.logging_setup as logging_setup_mod

    reset_concurrent_log_handlers(module=logging_setup_mod)
    root_logger = logging.getLogger()
    logger_a = logging.getLogger("greenfloor.manager")
    logger_b = logging.getLogger("greenfloor.daemon")
    try:
        handler_a = initialize_service_file_logging(
            service_name="manager",
            home_dir=str(tmp_path),
            log_level="INFO",
            service_logger=logger_a,
        )
        handler_b = initialize_service_file_logging(
            service_name="daemon",
            home_dir=str(tmp_path),
            log_level="INFO",
            service_logger=logger_b,
        )
        rotating_handlers = [
            handler
            for handler in root_logger.handlers
            if handler.__class__.__name__.endswith("RotatingFileHandler")
        ]
        assert handler_a is handler_b
        assert len(rotating_handlers) == 1
    finally:
        reset_concurrent_log_handlers(module=logging_setup_mod)

from __future__ import annotations

from concurrent_log_handler import ConcurrentRotatingFileHandler

import greenfloor.logging_setup as _logging_setup


def reset_concurrent_log_handlers(*, module) -> None:
    root_logger = module.logging.getLogger()
    for handler in list(root_logger.handlers):
        if isinstance(handler, ConcurrentRotatingFileHandler):
            root_logger.removeHandler(handler)
            handler.close()
    # Legacy module-level flags (kept for backward compat with older tests).
    if hasattr(module, "_manager_file_logger_initialized"):
        module._manager_file_logger_initialized = False
    if hasattr(module, "_daemon_file_logger_initialized"):
        module._daemon_file_logger_initialized = False
    if hasattr(module, "_daemon_file_log_handler"):
        module._daemon_file_log_handler = None
    # Reset the shared logging_setup registry so handlers are re-created.
    _logging_setup._initialized_services.clear()

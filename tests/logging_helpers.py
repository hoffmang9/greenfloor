from __future__ import annotations

from concurrent_log_handler import ConcurrentRotatingFileHandler


def reset_concurrent_log_handlers(*, module) -> None:
    root_logger = module.logging.getLogger()
    for handler in list(root_logger.handlers):
        if isinstance(handler, ConcurrentRotatingFileHandler):
            root_logger.removeHandler(handler)
            handler.close()
    if hasattr(module, "_manager_file_logger_initialized"):
        module._manager_file_logger_initialized = False
    if hasattr(module, "_daemon_file_logger_initialized"):
        module._daemon_file_logger_initialized = False
    if hasattr(module, "_daemon_file_log_handler"):
        module._daemon_file_log_handler = None

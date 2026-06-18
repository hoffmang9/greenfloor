"""Backward-compatible re-exports; prefer ``lib.manager_subprocess``."""

from __future__ import annotations

from lib.manager_subprocess import parse_json_output, run_manager

__all__ = ["parse_json_output", "run_manager"]

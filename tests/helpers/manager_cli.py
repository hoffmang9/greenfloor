"""Backward-compatible re-exports; prefer ``greenfloor.manager_subprocess``."""

from __future__ import annotations

from greenfloor.manager_subprocess import parse_json_output, run_manager

__all__ = ["parse_json_output", "run_manager"]

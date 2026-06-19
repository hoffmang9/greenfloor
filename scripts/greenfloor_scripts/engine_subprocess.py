"""Subprocess bridge to ``greenfloor-engine`` JSON CLI commands."""

from __future__ import annotations

import json
import subprocess
from typing import Any

from greenfloor_scripts.binaries import (
    GreenfloorEngineBinaryError,
    resolve_greenfloor_engine_binary,
)


def run_engine_json(argv: list[str]) -> Any:
    """Run ``greenfloor-engine`` with ``--json`` and parse stdout.

    Uses ``build_if_missing=False`` so scripts fail fast when binaries are absent;
    resolve binaries explicitly via ``binaries.resolve_*`` when auto-build is desired.
    """
    try:
        binary = resolve_greenfloor_engine_binary(build_if_missing=False)
    except GreenfloorEngineBinaryError as exc:
        raise RuntimeError(f"engine_cli_binary_unavailable: {exc}") from exc
    cmd = [str(binary), *argv, "--json"]
    result = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "").strip()
        raise RuntimeError(f"engine_cli_failed:{detail}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError("engine_cli_invalid_json") from exc

"""Subprocess bridge to ``greenfloor-engine`` JSON CLI commands."""

from __future__ import annotations

import json
import subprocess
from typing import Any

from greenfloor_scripts.binaries import (
    GreenfloorEngineBinaryError,
    resolve_greenfloor_engine_binary,
)

ENGINE_CLI_FAILED_PREFIX = "engine_cli_failed:"


def engine_cli_error_detail(exc: Exception) -> str | None:
    message = str(exc).strip()
    if not message.startswith(ENGINE_CLI_FAILED_PREFIX):
        return None
    detail = message[len(ENGINE_CLI_FAILED_PREFIX) :].strip()
    return detail or None


def structured_cli_error_from_detail(detail: str) -> tuple[str, bool | None]:
    if not detail.startswith("{"):
        return detail, None
    try:
        payload = json.loads(detail)
    except json.JSONDecodeError:
        return detail, None
    if not isinstance(payload, dict):
        return detail, None
    error = str(payload.get("error") or "").strip()
    retryable = payload.get("retryable")
    if isinstance(retryable, bool):
        return error or detail, retryable
    return detail, None


def is_retryable_engine_cli_error(exc: Exception) -> bool:
    """Return whether a script should retry based on engine JSON stderr.

    Retry classification is canonical in ``greenfloor-engine`` (``cli_util``);
    Python reads the ``retryable`` field from JSON error payloads only.
    """
    detail = engine_cli_error_detail(exc)
    if detail is None:
        return False
    _error_text, retryable = structured_cli_error_from_detail(detail)
    return retryable is True


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
        raise RuntimeError(f"{ENGINE_CLI_FAILED_PREFIX}{detail}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError("engine_cli_invalid_json") from exc

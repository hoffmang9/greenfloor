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

# Substrings observed in ``SignerError::Coinset`` / reqwest transport failures surfaced
# as ``engine_cli_failed:error: coinset error: …`` from ``greenfloor-engine`` stderr.
_RETRYABLE_COINSET_TRANSPORT_MARKERS = (
    "operation timed out",
    "connection refused",
    "connection reset",
    "remote end closed connection",
    "error sending request",
    "temporary failure",
    "temporarily unavailable",
    "broken pipe",
    "http status server error (502",
    "http status server error (503",
    "http status server error (504",
    "http status client error (429",
    "too many requests",
    "bad gateway",
    "service unavailable",
    "error decoding response body",
    "ssl",
    "handshake",
    "cloudflare",
)

_NON_RETRYABLE_ENGINE_CLI_MARKERS = (
    "error: parse body json:",
    "coinset endpoint is required",
    "invalid hex:",
)


def engine_cli_error_detail(exc: Exception) -> str | None:
    message = str(exc).strip()
    if not message.startswith(ENGINE_CLI_FAILED_PREFIX):
        return None
    detail = message[len(ENGINE_CLI_FAILED_PREFIX) :].strip()
    return detail or None


def is_retryable_engine_cli_error(exc: Exception) -> bool:
    detail = engine_cli_error_detail(exc)
    if detail is None:
        return False
    detail_lower = detail.lower()
    if any(marker in detail_lower for marker in _NON_RETRYABLE_ENGINE_CLI_MARKERS):
        return False
    if "coinset error:" not in detail_lower:
        return False
    return any(marker in detail_lower for marker in _RETRYABLE_COINSET_TRANSPORT_MARKERS)


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
